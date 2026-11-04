//! Passthrough LTO.
//!
//! Cranelift has no cross-module optimizations, so LTO only has to produce what rustc expects from
//! it: when LTO is enabled, rustc leaves the object files of all upstream rlibs out of the link and
//! expects them to be part of the LTO output. Thin LTO re-emits each upstream object file
//! unchanged. Fat LTO additionally merges all object files into a single relocatable object.

use std::collections::HashSet;
use std::ffi::CString;
use std::fs::{self, File};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use object::read::archive::ArchiveFile;
use rustc_codegen_ssa::back::lto::{SerializedModule, ThinModule, ThinShared};
use rustc_codegen_ssa::back::rmeta_link;
use rustc_codegen_ssa::back::write::{CodegenContext, FatLtoInput, ThinLtoInput};
use rustc_codegen_ssa::traits::{ModuleBufferMethods, WriteBackendMethods};
use rustc_codegen_ssa::{CompiledModule, ModuleKind};
use rustc_data_structures::memmap::Mmap;
use rustc_errors::DiagCtxtHandle;
use rustc_middle::dep_graph::WorkProduct;
use rustc_session::Session;
use rustc_session::config::OutputType;

use crate::driver::aot::AotDriver;

/// The magic numbers of raw LLVM bitcode and of the bitcode wrapper format.
const BITCODE_MAGICS: [&[u8]; 2] = [b"BC\xC0\xDE", b"\xDE\xC0\x17\x0B"];

pub(crate) enum ModuleBuffer {
    /// The object file of a codegen unit followed by its global asm object file, if any. This is
    /// also what is written as pre-LTO artifact for incremental compilation.
    Local(Vec<u8>),
    /// An object file of an upstream rlib. Kept mapped to avoid copying all upstream objects into
    /// memory.
    Upstream { rlib: Arc<Mmap>, range: Range<usize> },
}

impl ModuleBuffer {
    pub(crate) fn new(object: &[u8], global_asm_object: &[u8]) -> Self {
        let mut data = Vec::with_capacity(8 + object.len() + global_asm_object.len());
        data.extend_from_slice(&(object.len() as u64).to_le_bytes());
        data.extend_from_slice(object);
        data.extend_from_slice(global_asm_object);
        ModuleBuffer::Local(data)
    }

    /// Returns the object file and the global asm object file, which is empty if there is none.
    fn split(&self) -> (&[u8], &[u8]) {
        match self {
            ModuleBuffer::Local(data) => {
                let (object_len, data) = data.split_at(8);
                data.split_at(u64::from_le_bytes(object_len.try_into().unwrap()) as usize)
            }
            ModuleBuffer::Upstream { .. } => (self.data(), &[]),
        }
    }
}

impl ModuleBufferMethods for ModuleBuffer {
    fn data(&self) -> &[u8] {
        match self {
            ModuleBuffer::Local(data) => data,
            ModuleBuffer::Upstream { rlib, range } => &rlib[range.clone()],
        }
    }
}

pub(crate) fn run_thin_lto(
    cgcx: &CodegenContext,
    dcx: DiagCtxtHandle<'_>,
    each_linked_rlib_for_lto: &[PathBuf],
    inputs: Vec<ThinLtoInput<AotDriver>>,
) -> (Vec<ThinModule<AotDriver>>, Vec<WorkProduct>) {
    let mut names = Vec::new();
    let mut modules = Vec::new();

    for input in inputs {
        match input {
            ThinLtoInput::Red { name, buffer } => {
                names.push(CString::new(name).unwrap());
                modules.push(buffer);
            }
            ThinLtoInput::Green { wp, bitcode_path } => {
                names.push(CString::new(wp.cgu_name).unwrap());
                modules.push(SerializedModule::Local(read_serialized(dcx, &bitcode_path)));
            }
        }
    }

    for_each_upstream_object(cgcx, dcx, each_linked_rlib_for_lto, |name, buffer| {
        names.push(CString::new(name).unwrap());
        modules.push(SerializedModule::Local(buffer));
    });

    let shared = Arc::new(ThinShared { data: (), module_names: names, modules });
    let thin_modules = (0..shared.modules.len())
        .map(|idx| ThinModule { shared: Arc::clone(&shared), idx })
        .collect();

    (thin_modules, vec![])
}

pub(crate) fn optimize_and_codegen_thin(
    cgcx: &CodegenContext,
    dcx: DiagCtxtHandle<'_>,
    thin: ThinModule<AotDriver>,
) -> CompiledModule {
    let SerializedModule::Local(buffer) = &thin.shared.modules[thin.idx] else { unreachable!() };
    write_module(cgcx, dcx, thin.name(), "", buffer)
}

pub(crate) fn optimize_and_codegen_fat_lto(
    sess: &Session,
    cgcx: &CodegenContext,
    dcx: DiagCtxtHandle<'_>,
    each_linked_rlib_for_lto: &[PathBuf],
    modules: Vec<FatLtoInput<AotDriver>>,
) -> CompiledModule {
    let mut inputs = Vec::new();
    // Name the merged module after one of the local modules, choosing the first one by name to
    // keep the output deterministic.
    let mut name = None::<String>;
    for module in modules {
        let (module_name, buffer) = match module {
            FatLtoInput::InMemory(module) => {
                (module.name, AotDriver::serialize_module(module.module_llvm, false))
            }
            FatLtoInput::Serialized { name, bitcode_path } => {
                (name, read_serialized(dcx, &bitcode_path))
            }
        };
        let module = write_module(cgcx, dcx, &module_name, "lto.", &buffer);
        inputs.extend(module.object);
        inputs.extend(module.global_asm_object);
        if name.as_ref().is_none_or(|name| module_name < *name) {
            name = Some(module_name);
        }
    }
    let name = name.unwrap();

    for_each_upstream_object(cgcx, dcx, each_linked_rlib_for_lto, |name, buffer| {
        let module = write_module(cgcx, dcx, name, "lto.", &buffer);
        inputs.extend(module.object);
        inputs.extend(module.global_asm_object);
    });

    let object = cgcx.output_filenames.temp_path_for_cgu(OutputType::Object, &name);
    link_relocatable(sess, dcx, cgcx, &name, &inputs, &object);
    for input in inputs {
        drop(fs::remove_file(input));
    }

    CompiledModule {
        name,
        kind: ModuleKind::Regular,
        object: Some(object),
        global_asm_object: None,
        dwarf_object: None,
        bytecode: None,
        assembly: None,
        llvm_ir: None,
    }
}

fn read_serialized(dcx: DiagCtxtHandle<'_>, path: &Path) -> ModuleBuffer {
    let data = fs::read(path)
        .unwrap_or_else(|err| dcx.fatal(format!("failed to read `{}`: {err}", path.display())));
    ModuleBuffer::Local(data)
}

/// Calls `f` with the name and contents of each object file of the upstream crates taking part in
/// LTO.
fn for_each_upstream_object(
    cgcx: &CodegenContext,
    dcx: DiagCtxtHandle<'_>,
    each_linked_rlib_for_lto: &[PathBuf],
    mut f: impl FnMut(&str, ModuleBuffer),
) {
    for path in each_linked_rlib_for_lto {
        let file = File::open(path)
            .unwrap_or_else(|err| dcx.fatal(format!("failed to open `{}`: {err}", path.display())));
        // SAFETY: The rlib is not modified while rustc is running.
        let rlib = unsafe { Mmap::map(file) }
            .unwrap_or_else(|err| dcx.fatal(format!("failed to map `{}`: {err}", path.display())));
        let rlib = Arc::new(rlib);
        let archive = ArchiveFile::parse(&**rlib).unwrap_or_else(|err| {
            dcx.fatal(format!("failed to parse rlib `{}`: {err}", path.display()))
        });
        let Some(rmeta_link) = rmeta_link::read_from_data(&rlib, path, cgcx.target_is_like_aix)
        else {
            dcx.fatal(format!("rlib `{}` has no link metadata", path.display()));
        };
        let rust_object_files =
            rmeta_link.rust_object_files.iter().map(String::as_str).collect::<HashSet<_>>();

        for member in archive.members() {
            let member = member.unwrap_or_else(|err| {
                dcx.fatal(format!("failed to read rlib `{}`: {err}", path.display()))
            });
            let Ok(name) = str::from_utf8(member.name()) else { continue };
            let name = name.trim();
            if !rust_object_files.contains(name) {
                continue;
            }
            let (start, len) = member.file_range();
            let range = start as usize..(start + len) as usize;
            if BITCODE_MAGICS.iter().any(|magic| rlib[range.clone()].starts_with(magic)) {
                dcx.fatal(format!(
                    "`{name}` in `{}` contains LLVM bitcode, which cannot be linked by \
                     rustc_codegen_cranelift. Was it built with `-Clinker-plugin-lto`?",
                    path.display()
                ));
            }
            f(name, ModuleBuffer::Upstream { rlib: Arc::clone(&rlib), range });
        }
    }
}

/// Writes the object files of a module to the temporary files of the codegen unit `name`, using
/// `prefix` to tell them apart from the final output of the codegen unit.
fn write_module(
    cgcx: &CodegenContext,
    dcx: DiagCtxtHandle<'_>,
    name: &str,
    prefix: &str,
    buffer: &ModuleBuffer,
) -> CompiledModule {
    let (object, global_asm_object) = buffer.split();

    let object_path = cgcx.output_filenames.temp_path_ext_for_cgu(&format!("{prefix}o"), name);
    write_file(dcx, &object_path, object);

    let global_asm_object_path = (!global_asm_object.is_empty()).then(|| {
        let path = cgcx.output_filenames.temp_path_ext_for_cgu(&format!("{prefix}asm.o"), name);
        write_file(dcx, &path, global_asm_object);
        path
    });

    CompiledModule {
        name: name.to_owned(),
        kind: ModuleKind::Regular,
        object: Some(object_path),
        global_asm_object: global_asm_object_path,
        dwarf_object: None,
        bytecode: None,
        assembly: None,
        llvm_ir: None,
    }
}

fn write_file(dcx: DiagCtxtHandle<'_>, path: &Path, data: &[u8]) {
    fs::write(path, data)
        .unwrap_or_else(|err| dcx.fatal(format!("failed to write `{}`: {err}", path.display())));
}

/// Merges `inputs` into the single relocatable object `output`.
fn link_relocatable(
    sess: &Session,
    dcx: DiagCtxtHandle<'_>,
    cgcx: &CodegenContext,
    name: &str,
    inputs: &[PathBuf],
    output: &Path,
) {
    // The list of inputs can be too long for the command line.
    let list_path = cgcx.output_filenames.temp_path_ext_for_cgu("lto.list", name);
    let mut list = String::new();
    for input in inputs {
        let input = input.to_str().expect("temporary file path should be valid UTF-8");
        if sess.target.is_like_darwin {
            // `-filelist` takes one unquoted path per line.
            list.push_str(input);
        } else {
            list.push('"');
            list.push_str(&input.replace('\\', "\\\\").replace('"', "\\\""));
            list.push('"');
        }
        list.push('\n');
    }
    write_file(dcx, &list_path, list.as_bytes());

    let linker = crate::toolchain::get_toolchain_binary(sess, "ld");
    let mut cmd = Command::new(&linker);
    cmd.arg("-r").arg("-o").arg(output);
    if sess.target.is_like_darwin {
        cmd.arg("-filelist").arg(&list_path);
    } else {
        // Cranelift gives all function sections the same name, which would otherwise be merged
        // into a single section, preventing `--gc-sections` from removing unused functions. Only
        // GNU ld can keep them apart.
        if is_gnu_ld(&linker) {
            cmd.arg("--unique=.text.subsection");
        }
        cmd.arg(format!("@{}", list_path.display()));
    }
    let result = cmd.output();
    drop(fs::remove_file(&list_path));

    match result {
        Ok(result) if result.status.success() => {}
        Ok(result) => dcx.fatal(format!(
            "failed to merge object files for LTO with `{}`: {}\n{}",
            linker.display(),
            result.status,
            String::from_utf8_lossy(&result.stderr),
        )),
        Err(err) => dcx.fatal(format!("failed to run `{}`: {err}", linker.display())),
    }
}

fn is_gnu_ld(linker: &Path) -> bool {
    Command::new(linker)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.stdout.starts_with(b"GNU ld"))
}
