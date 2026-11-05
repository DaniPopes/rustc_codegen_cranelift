/// Configuration of cg_clif as passed in through `-Cllvm-args` and various env vars.
#[derive(Debug)]
pub struct BackendConfig {
    /// Should the crate be AOT compiled or JIT executed.
    ///
    /// Defaults to AOT compilation. Can be set using `-Cllvm-args=jit-mode`.
    pub jit_mode: bool,

    /// When JIT mode is enable pass these arguments to the program.
    ///
    /// Defaults to the value of `CG_CLIF_JIT_ARGS`.
    pub jit_args: Vec<String>,
}

impl BackendConfig {
    /// Parse the configuration passed in using `-Cllvm-args`.
    ///
    /// Options starting with `-` are ignored. All other options are cg_clif options.
    pub fn from_opts(opts: &[String]) -> Result<Self, String> {
        let mut config = BackendConfig {
            jit_mode: false,
            jit_args: match std::env::var("CG_CLIF_JIT_ARGS") {
                Ok(args) => args.split(' ').map(|arg| arg.to_string()).collect(),
                Err(std::env::VarError::NotPresent) => vec![],
                Err(std::env::VarError::NotUnicode(s)) => {
                    panic!("CG_CLIF_JIT_ARGS not unicode: {:?}", s);
                }
            },
        };

        for opt in opts {
            if opt.starts_with('-') {
                // Options starting with `-` are LLVM options, which are handled by LLVM when the
                // crate is delegated to it. (And ignored otherwise. E.g. rust's build system sets
                // `-import-instr-limit` even when testing cg_clif.)
                continue;
            }
            match &**opt {
                "jit-mode" => config.jit_mode = true,
                _ => return Err(format!("Unknown option `{}`", opt)),
            }
        }

        Ok(config)
    }
}
