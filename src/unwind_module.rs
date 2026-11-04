use cranelift_codegen::Context;
use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::Signature;
use cranelift_codegen::isa::{TargetFrontendConfig, TargetIsa};
use cranelift_module::{
    DataDescription, DataId, FuncId, FuncOrDataId, Linkage, Module, ModuleDeclarations,
    ModuleReloc, ModuleResult,
};
use cranelift_object::{ObjectModule, ObjectProduct};
use target_lexicon::BinaryFormat;

use crate::UnwindContext;

/// A wrapper around a [Module] which adds any defined function to the [UnwindContext] and resolves
/// LLVM's `\x01` verbatim symbol name prefix.
pub(crate) struct UnwindModule<T> {
    pub(crate) module: T,
    unwind_context: UnwindContext,
    /// Whether `object` and the dynamic loader add a `_` prefix to every symbol name.
    underscore_prefix: bool,
}

impl<T: Module> UnwindModule<T> {
    pub(crate) fn new(mut module: T, eh_personality: &str, pic_eh_frame: bool) -> Self {
        let unwind_context = UnwindContext::new(&mut module, eh_personality, pic_eh_frame);
        let underscore_prefix = module.isa().triple().binary_format == BinaryFormat::Macho;
        UnwindModule { module, unwind_context, underscore_prefix }
    }

    /// Turns a `\x01`-prefixed name, which names the symbol exactly, into the name that becomes
    /// that symbol once the `_` prefix is added again. Returns `name` unchanged if it isn't
    /// verbatim or if the prefix can't be added to a name without it.
    fn strip_verbatim_prefix<'a>(&self, name: &'a str) -> &'a str {
        let Some(verbatim) = name.strip_prefix('\x01') else { return name };
        if self.underscore_prefix { verbatim.strip_prefix('_').unwrap_or(name) } else { verbatim }
    }
}

impl UnwindModule<ObjectModule> {
    pub(crate) fn finish(self) -> ObjectProduct {
        // Verbatim names that `strip_verbatim_prefix` left alone get the prefix added by `object`
        // regardless, so they are renamed once the object exists.
        let is_verbatim = |name: &String| self.underscore_prefix && name.starts_with('\x01');
        let declarations = self.module.declarations();
        let functions = declarations
            .get_functions()
            .filter(|(_, decl)| decl.name.as_ref().is_some_and(is_verbatim))
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        let data_objects = declarations
            .get_data_objects()
            .filter(|(_, decl)| decl.name.as_ref().is_some_and(is_verbatim))
            .map(|(id, _)| id)
            .collect::<Vec<_>>();

        let mut product = self.module.finish();
        self.unwind_context.emit(&mut product);

        let symbols = functions
            .into_iter()
            .filter_map(|id| product.functions[id])
            .chain(data_objects.into_iter().filter_map(|id| product.data_objects[id]));
        for (symbol, _) in symbols {
            // Drop the `_` prefix and the `\x01`.
            product.object.symbol_mut(symbol).name.drain(..2);
        }
        product
    }
}

#[cfg(feature = "jit")]
impl UnwindModule<cranelift_jit::JITModule> {
    pub(crate) fn finalize_definitions(mut self) -> cranelift_jit::JITModule {
        self.module.finalize_definitions().unwrap();
        unsafe { self.unwind_context.register_jit(&self.module) };
        self.module
    }
}

impl<T: Module> Module for UnwindModule<T> {
    fn isa(&self) -> &dyn TargetIsa {
        self.module.isa()
    }

    fn declarations(&self) -> &ModuleDeclarations {
        self.module.declarations()
    }

    fn get_name(&self, name: &str) -> Option<FuncOrDataId> {
        self.module.get_name(self.strip_verbatim_prefix(name))
    }

    fn target_config(&self) -> TargetFrontendConfig {
        self.module.target_config()
    }

    fn declare_function(
        &mut self,
        name: &str,
        linkage: Linkage,
        signature: &Signature,
    ) -> ModuleResult<FuncId> {
        let name = self.strip_verbatim_prefix(name);
        self.module.declare_function(name, linkage, signature)
    }

    fn declare_anonymous_function(&mut self, signature: &Signature) -> ModuleResult<FuncId> {
        self.module.declare_anonymous_function(signature)
    }

    fn declare_data(
        &mut self,
        name: &str,
        linkage: Linkage,
        writable: bool,
        tls: bool,
    ) -> ModuleResult<DataId> {
        let name = self.strip_verbatim_prefix(name);
        self.module.declare_data(name, linkage, writable, tls)
    }

    fn declare_anonymous_data(&mut self, writable: bool, tls: bool) -> ModuleResult<DataId> {
        self.module.declare_anonymous_data(writable, tls)
    }

    fn define_function_with_control_plane(
        &mut self,
        func: FuncId,
        ctx: &mut Context,
        ctrl_plane: &mut ControlPlane,
    ) -> ModuleResult<()> {
        self.module.define_function_with_control_plane(func, ctx, ctrl_plane)?;
        self.unwind_context.add_function(&mut self.module, func, ctx);
        Ok(())
    }

    fn define_function_bytes(
        &mut self,
        _func_id: FuncId,
        _alignment: u64,
        _bytes: &[u8],
        _relocs: &[ModuleReloc],
    ) -> ModuleResult<()> {
        unimplemented!()
    }

    fn define_data(&mut self, data_id: DataId, data: &DataDescription) -> ModuleResult<()> {
        self.module.define_data(data_id, data)
    }
}
