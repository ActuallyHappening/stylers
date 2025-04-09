//! ## Feature flags
#![doc = document_features::document_features!()]
//!
//! ## Usage
//! An example build.rs
//! ```rust
#![doc = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/build.rs"))]
//! ```

pub use stylers_macro::style;
pub use stylers_macro::style_sheet;
pub use stylers_macro::style_sheet_str;
pub use stylers_macro::style_str;

#[cfg(feature = "build")]
pub use build::*;
#[cfg(feature = "build")]
mod build {
    use camino::{Utf8Path, Utf8PathBuf};
    use color_eyre::Section;
    use color_eyre::eyre::{WrapErr as _, bail};
    use glob::glob;

    use std::fs::File;
    use std::io::{self, Write};
    use std::num::Saturating;
    use std::{borrow::Borrow, env::current_dir, fs};
    use stylers_core::Class;
    use stylers_core::{from_str, from_ts};
    use syn::{Expr, Item, Stmt};
    #[allow(unused_imports)]
    use tracing::{debug, error, info, trace, warn};

    #[cfg(feature = "build-script")]
    macro_rules! p {
    ($($tokens: tt)*) => {
        println!("cargo::warning={}", format!($($tokens)*))
    }
}
    #[cfg(not(feature = "build-script"))]
    macro_rules! p {
        ($($tokens: tt)*) => {};
    }

    pub struct BuildParams {
        output_path: Utf8PathBuf,
        search_dir: Utf8PathBuf,
    }

    impl BuildParams {
        pub fn builder() -> BuildParamsBuilder {
            BuildParamsBuilder::default()
        }
    }

    #[derive(Default)]
    pub struct BuildParamsBuilder {
        output_path: Option<Utf8PathBuf>,
        search_dir: Option<Utf8PathBuf>,
    }

    impl BuildParamsBuilder {
        /// File path to output collated .css to
        pub fn with_output_path(self, path: Utf8PathBuf) -> color_eyre::Result<Self> {
            if !path.is_file() {
                // tries to create recursive dirs to path
                let mut path2 = path.clone();
                path2.pop();
                match std::fs::create_dir_all(path2) {
                    Ok(_) => {}
                    Err(err) => {
                        warn!(
                            ?err,
                            "Was trying to create output directory for the specified output path {:?}, but failed",
                            path
                        );
                    }
                }
                std::fs::File::create_new(&path)
                    .wrap_err(format!("Couldn't create output file at path {:?}", path))?;
            }
            Ok(Self {
                output_path: Some(path),
                search_dir: self.search_dir,
            })
        }

        /// Directory path to search .rs files in
        pub fn with_search_dir(self, path: Utf8PathBuf) -> color_eyre::Result<Self> {
            if !path.is_dir() {
                bail!(
                    "Search dir {:?} does not exist, or is not a directory path",
                    path
                )
            } else {
                Ok(Self {
                    output_path: self.output_path,
                    search_dir: Some(path),
                })
            }
        }

        /// Will error if appropriate defaults were not provided,
        /// or paths were not utf8 encoded
        pub fn finish(mut self) -> color_eyre::Result<BuildParams> {
            let output_path: Utf8PathBuf = match &self.output_path {
                Some(output_path) => output_path.clone(),
                None => {
                    let default = current_dir()?
                        .join("target")
                        .join("stylers_out.css")
                        .try_into()?;
                    self = self
                        .with_output_path(default)
                        .wrap_err("Couldn't use default output path")?;
                    self.output_path.as_ref().unwrap().clone()
                }
            };
            let search_dir: Utf8PathBuf = match self.search_dir {
                Some(search_dir) => search_dir,
                None => {
                    let default = current_dir()?.join("src").try_into()?;
                    self = self
                        .with_search_dir(default)
                        .wrap_err("Couldn't use default search dir")?;
                    self.search_dir.unwrap()
                }
            };
            Ok(BuildParams {
                output_path,
                search_dir,
            })
        }
    }

    /// Requires the `build` feature flag.
    /// Will search your local fs and compile the css snippets you have included
    pub fn build(build_params: BuildParams) -> color_eyre::Result<()> {
        // if called by itself, this will make error messages pretty :)
        color_eyre::install().ok();

        let pattern = format!("{}/**/*.rs", build_params.search_dir);

        info!(search_pattern = %pattern, output_file = %build_params.output_path, "Building stylers css output");
        let mut files_counter = Saturating(0u128);
        let mut macros_couter = Saturating(0u32);

        let mut output_css = String::from("");
        p!(
            "{}",
            "===============================Stylers debug output start==============================="
        );
        for file in glob(&pattern).unwrap() {
            let file = file.unwrap();
            let content = fs::read_to_string(&file)
                .wrap_err("Failed to read .rs file")
                .note(format!("File path: {:?}", file))
                .note("Skipping this file");
            //
            let content = match content {
                Ok(content) => {
                    debug!(?file, "Processing file");
                    content
                }
                Err(err) => {
                    println!("cargo::warning={}", err);
                    warn!(
                        ?err,
                        ?file,
                        %pattern,
                        "Glob pattern matched a file that can't be read for some reason?"
                    );
                    continue;
                }
            };
            let ast =
                syn::parse_file(&content).wrap_err("Couldn't parse file as syn token stream")?;

            files_counter += 1;

            // check the each item in the *.rs file
            for item in ast.items {
                // check if the item is of type Function.
                if let Item::Fn(fn_def) = item {
                    let _componet_name = &fn_def.sig.ident;
                    // check each statement in the function
                    for stmt in fn_def.block.stmts {
                        // check if any of the statment is of the form `let any_valid_variable = style!{}`
                        if let Stmt::Local(let_bin) = stmt {
                            if let Some(init) = let_bin.init {
                                if let Expr::Macro(expr_mac) = init.expr.borrow() {
                                    if let Some(path_seg) = expr_mac.mac.path.segments.last() {
                                        let macro_name = path_seg.ident.clone().to_string();
                                        // p!("macro_name:{:?}", macro_name);

                                        if macro_name == *"style" {
                                            debug!(?file, "Processing `style` macro in file");
                                            macros_couter += 1;
                                            let ts = expr_mac.mac.tokens.clone();
                                            let class = Class::rand_class_from_seed(ts.to_string());
                                            let token_stream = ts.into_iter();
                                            let (scoped_css, _) =
                                                from_ts(token_stream, &class, false);
                                            output_css += &scoped_css;
                                            continue;
                                        }

                                        if macro_name == *"style_sheet" {
                                            debug!(?file, "Processing `style_sheet` macro in file");
                                            macros_couter += 1;
                                            let ts = expr_mac.mac.tokens.clone();
                                            let file_path = ts.to_string();
                                            let file_path = file_path.trim_matches('"');
                                            let css_content = std::fs::read_to_string(file_path)
                                                .expect("Expected to read file");

                                            let class = Class::rand_class_from_seed(
                                                css_content.to_string(),
                                            );
                                            let style = from_str(&css_content, &class);
                                            output_css += &style;
                                            continue;
                                        }

                                        if macro_name.contains("style") {
                                            trace!(
                                                ?macro_name,
                                                note = "This macro was not a known `stylers` macro",
                                                suggestion = "Use either `style` or `style_sheet`"
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        //todo: other than let statements cover that other way style! macro can instantiated.
                    }
                }
            }
        }

        write_css(&build_params.output_path, &output_css).wrap_err("Error writing output CSS")?;
        // .unwrap_or_else(|e| p!("Problem creating output file: {}", e.to_string()));

        p!(
            "{}",
            "===============================Stylers debug output end==============================="
        );
        info!(files_read = %files_counter, macros_processed = %macros_couter, "Finished processing stylers");
        Ok(())
    }

    /// Writes the styles in its own file and appends itself to the main.css file
    fn write_css(out_path: &Utf8Path, content: &str) -> io::Result<()> {
        let mut buffer = File::create(out_path)?;
        buffer.write_all(content.as_bytes())?;
        buffer.flush()?;

        Ok(())
    }
}
