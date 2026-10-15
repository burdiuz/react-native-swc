//! JSON-in / JSON-out wrapper around SWC for use from a React Native native module.
mod analyze;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use swc_core::base::{
    config::{JsMinifyOptions, Options},
    try_with_handler, Compiler, HandlerOpts, PrintArgs,
};
use swc_core::common::{
    comments::SingleThreadedComments,
    errors::{ColorConfig, Handler},
    sync::Lrc,
    FileName, FilePathMapping, SourceFile, SourceMap, GLOBALS,
};
use swc_error_reporters::TWithDiagnosticArray;
use swc_core::ecma::ast::{noop_pass, EsVersion, Program};
use swc_core::ecma::codegen::Config as CodegenConfig;
use swc_core::ecma::transforms::base::fixer::fixer;
use swc_core::ecma::visit::{visit_pass, VisitMut, VisitMutWith};
use swc_core::common::{Span, DUMMY_SP};

type Output = Result<String, String>;

/// Runs `f` with a fresh compiler, the parsed-source handle and SWC's error handler.
fn run<T>(
    source: &str,
    filename: &str,
    f: impl FnOnce(&Compiler, &Handler, Lrc<SourceFile>) -> Result<T, anyhow::Error>,
) -> Result<T, String> {
    let cm = Arc::new(SourceMap::new(FilePathMapping::empty()));
    let compiler = Compiler::new(cm.clone());
    GLOBALS
        .set(&Default::default(), || {
            try_with_handler(
                cm.clone(),
                HandlerOpts { color: ColorConfig::Never, skip_filename: false },
                |handler| {
                    let fm = cm.new_source_file(
                        Arc::new(FileName::Custom(filename.to_string())),
                        source.to_string(),
                    );
                    f(&compiler, handler, fm)
                },
            )
        })
        .map_err(|e| format_diagnostics(&cm, &e))
}

/// Renders SWC diagnostics as `file:line:col: message`, one per line (1-based line and column).
/// Falls back to the error's own message when SWC reported no located diagnostics.
fn format_diagnostics(cm: &SourceMap, err: &TWithDiagnosticArray<anyhow::Error>) -> String {
    let lines: Vec<String> = err
        .diagnostics()
        .iter()
        .map(|d| {
            let message = d.message();
            match d.span.primary_span().filter(|s| !s.is_dummy()) {
                Some(span) => {
                    let loc = cm.lookup_char_pos(span.lo);
                    format!("{}:{}:{}: {}", loc.file.name, loc.line, loc.col.0 + 1, message)
                }
                None => message,
            }
        })
        .collect();
    if lines.is_empty() {
        err.to_string()
    } else {
        lines.join("\n")
    }
}

fn to_json(result: Output) -> String {
    match result {
        Ok(json) => json,
        Err(error) => serde_json::json!({ "error": error }).to_string(),
    }
}

fn parse_options(options_json: &str) -> Result<Options, String> {
    let json = if options_json.trim().is_empty() { "{}" } else { options_json };
    serde_json::from_str(json).map_err(|e| format!("invalid options: {e}"))
}

/// Transforms `source`; returns `{"code","map","imports","exports"}` or `{"error"}` as JSON.
/// `imports`/`exports` describe the module after TypeScript stripping, before the module transform.
pub fn transform_json(source: &str, filename: &str, options_json: &str) -> String {
    to_json(transform(source, filename, options_json))
}

fn transform(source: &str, filename: &str, options_json: &str) -> Output {
    let opts = parse_options(options_json)?;
    let analysis = Rc::new(RefCell::new(analyze::Analysis::default()));
    let out = run(source, filename, |compiler, handler, fm| {
        compiler.process_js_with_custom_pass(
            fm,
            None,
            handler,
            &opts,
            SingleThreadedComments::default(),
            // Built eagerly but applied after TypeScript stripping (so elided imports are gone).
            |_| visit_pass(analyze::Collector::new(analysis.clone())),
            |_| noop_pass(),
        )
    })?;
    let a = analysis.take();
    Ok(serde_json::json!({ "code": out.code, "map": out.map, "imports": a.imports, "exports": a.exports })
        .to_string())
}

/// Minifies `source` (JsMinifyOptions as JSON); returns `{"code","map"}` or `{"error"}`.
pub fn minify_json(source: &str, filename: &str, options_json: &str) -> String {
    to_json(minify(source, filename, options_json))
}

fn minify(source: &str, filename: &str, options_json: &str) -> Output {
    let json = if options_json.trim().is_empty() { "{}" } else { options_json };
    let opts: JsMinifyOptions =
        serde_json::from_str(json).map_err(|e| format!("invalid options: {e}"))?;
    let out = run(source, filename, |compiler, handler, fm| {
        compiler.minify(fm, handler, &opts, Default::default())
    })?;
    Ok(serde_json::json!({ "code": out.code, "map": out.map }).to_string())
}

fn parse_program(compiler: &Compiler, handler: &Handler, fm: Lrc<SourceFile>, opts: &Options) -> Result<Program, anyhow::Error> {
    let jsc = &opts.config.jsc;
    compiler.parse_js(
        fm,
        handler,
        jsc.target.unwrap_or_default(),
        jsc.syntax.unwrap_or_default(),
        opts.config.is_module.unwrap_or_default(),
        None,
    )
}

/// Parses `source` (syntax / target / isModule are read from the SWC `Options` JSON, no transforms
/// run); returns `{"ast":<Program>}` or `{"error"}`.
pub fn parse_json(source: &str, filename: &str, options_json: &str) -> String {
    to_json(parse(source, filename, options_json))
}

fn parse(source: &str, filename: &str, options_json: &str) -> Output {
    let opts = parse_options(options_json)?;
    let ast = run(source, filename, |compiler, handler, fm| {
        let program = parse_program(compiler, handler, fm, &opts)?;
        Ok(serde_json::to_string(&program)?)
    })?;
    Ok(format!("{{\"ast\":{ast}}}"))
}

/// Static import/export analysis of the untransformed source; returns `{"imports","exports"}`
/// or `{"error"}`. TypeScript is not stripped, so unused value imports are still listed.
pub fn analyze_json(source: &str, filename: &str, options_json: &str) -> String {
    to_json(analyze_source(source, filename, options_json))
}

fn analyze_source(source: &str, filename: &str, options_json: &str) -> Output {
    let opts = parse_options(options_json)?;
    let a = run(source, filename, |compiler, handler, fm| {
        Ok(analyze::analyze(&parse_program(compiler, handler, fm, &opts)?))
    })?;
    Ok(serde_json::json!({ "imports": a.imports, "exports": a.exports }).to_string())
}

/// Prints an AST (as produced by `parse_json`) back to code. Options JSON:
/// `{"target":"es2022","minify":false,"asciiOnly":false}`. Returns `{"code"}` or `{"error"}`.
/// Comments and source maps are not preserved (the AST carries neither).
pub fn print_json(ast_json: &str, options_json: &str) -> String {
    to_json(print(ast_json, options_json))
}

/// Parsed spans point into a `SourceMap` that no longer exists; codegen would look them up and
/// panic, so they are cleared before printing.
struct ClearSpans;

impl VisitMut for ClearSpans {
    fn visit_mut_span(&mut self, span: &mut Span) {
        *span = DUMMY_SP;
    }
}

fn print(ast_json: &str, options_json: &str) -> Output {
    let mut program: Program = serde_json::from_str(ast_json).map_err(|e| format!("invalid ast: {e}"))?;
    let json = if options_json.trim().is_empty() { "{}" } else { options_json };
    let o: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("invalid options: {e}"))?;
    let target: EsVersion = match o.get("target") {
        Some(t) => serde_json::from_value(t.clone()).map_err(|e| format!("invalid target: {e}"))?,
        None => EsVersion::latest(),
    };
    program.visit_mut_with(&mut ClearSpans);
    let flag = |k: &str| o.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let cm = Arc::new(SourceMap::new(FilePathMapping::empty()));
    let compiler = Compiler::new(cm);
    let out = GLOBALS
        .set(&Default::default(), || {
            // `fixer` re-adds parentheses a hand-edited AST may be missing.
            let program = program.apply(fixer(None));
            compiler.print(
                &program,
                PrintArgs {
                    codegen_config: CodegenConfig::default()
                        .with_target(target)
                        .with_minify(flag("minify"))
                        .with_ascii_only(flag("asciiOnly")),
                    ..Default::default()
                },
            )
        })
        .map_err(|e| format!("{e:#}"))?;
    Ok(serde_json::json!({ "code": out.code }).to_string())
}

#[cfg(target_os = "android")]
mod android {
    use jni::objects::{JClass, JString};
    use jni::sys::jstring;
    use jni::JNIEnv;

    fn call(env: &mut JNIEnv, args: &[&JString], f: impl FnOnce(&[String]) -> String + std::panic::UnwindSafe) -> jstring {
        let strings: Vec<String> = args
            .iter()
            .map(|s| env.get_string(s).map(|s| s.into()).unwrap_or_default())
            .collect();
        let out = std::panic::catch_unwind(|| f(&strings)).unwrap_or_else(|payload| {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            serde_json::json!({ "error": format!("swc panicked: {msg}") }).to_string()
        });
        env.new_string(out).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
    }

    macro_rules! entry3 {
        ($name:ident, $f:path) => {
            #[no_mangle]
            pub extern "system" fn $name(
                mut env: JNIEnv,
                _class: JClass,
                a: JString,
                b: JString,
                c: JString,
            ) -> jstring {
                call(&mut env, &[&a, &b, &c], |s| $f(&s[0], &s[1], &s[2]))
            }
        };
    }

    entry3!(Java_com_actualwave_swc_SwcModule_nativeTransform, super::transform_json);
    entry3!(Java_com_actualwave_swc_SwcModule_nativeMinify, super::minify_json);
    entry3!(Java_com_actualwave_swc_SwcModule_nativeParse, super::parse_json);
    entry3!(Java_com_actualwave_swc_SwcModule_nativeAnalyze, super::analyze_json);

    #[no_mangle]
    pub extern "system" fn Java_com_actualwave_swc_SwcModule_nativePrint(
        mut env: JNIEnv,
        _class: JClass,
        ast: JString,
        options: JString,
    ) -> jstring {
        call(&mut env, &[&ast, &options], |s| super::print_json(&s[0], &s[1]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPTS: &str = r#"{"jsc":{"parser":{"syntax":"typescript","tsx":true},"transform":{"react":{"runtime":"automatic"}},"keepClassNames":true,"externalHelpers":false},
      "env":{"targets":"chrome 110","include":["transform-classes","transform-async-to-generator",
      "transform-async-generator-functions","transform-class-properties","transform-private-methods",
      "transform-private-property-in-object","transform-class-static-block"]},
      "module":{"type":"commonjs"},"sourceMaps":true,"isModule":true,"swcrc":false,"configFile":false}"#;

    #[test]
    fn lowers_classes_async_arrows_and_jsx() {
        let src = "class A<T> { x: number = 1; async m() { return (async () => 1)(); } }\nexport default () => <div/>;";
        let out: serde_json::Value = serde_json::from_str(&transform_json(src, "a.tsx", OPTS)).unwrap();
        let code = out["code"].as_str().unwrap();
        assert!(!code.contains("class A"), "{code}");
        assert!(!code.contains("async function") && !code.contains("async ()") && !code.contains("class A"), "{code}");
        assert!(code.contains("jsx-runtime"), "{code}");
        assert!(!code.contains("@swc/helpers") && !code.contains("import "), "{code}");
        assert!(code.contains("require(") && code.contains("exports"), "{code}");
        assert!(out["map"].is_string());
    }

    #[test]
    fn minifies() {
        let src = "function add(first, second) { const unused = 1; return first + second; }\nconsole.log(add(1, 2));";
        let out: serde_json::Value =
            serde_json::from_str(&minify_json(src, "m.js", r#"{"compress":true,"mangle":true}"#)).unwrap();
        let code = out["code"].as_str().unwrap();
        assert!(code.len() < src.len(), "{code}");
        assert!(!code.contains("unused"), "{code}");
    }

    const MODULE: &str = r#"
import type { T } from 'types-only';
import { Unused } from './types';
import React, { useState as us } from 'react';
import * as ns from './ns';
import './side';
export * from './all';
export { x as y, z } from './re';
export const a = 1, { b } = {};
export function f(p: Unused) {}
export default class {}
const lazy = () => import('./lazy');
const r = require('./r');
const dyn = import(name);
console.log(React, us, ns);
"#;

    fn specs(v: &serde_json::Value) -> Vec<(String, String)> {
        v["imports"].as_array().unwrap().iter()
            .map(|d| (d["specifier"].as_str().unwrap().into(), d["kind"].as_str().unwrap().into()))
            .collect()
    }

    fn pair(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    #[test]
    fn transform_reports_imports_and_exports_after_ts_strip() {
        let v: serde_json::Value = serde_json::from_str(&transform_json(MODULE, "m.tsx", OPTS)).unwrap();
        // `types-only` (type import) and `./types` (only used as a type) are elided.
        assert_eq!(
            specs(&v),
            vec![
                pair("react", "import"),
                pair("./ns", "import"),
                pair("./side", "import"),
                pair("./all", "export-all"),
                pair("./re", "export-from"),
                pair("./lazy", "dynamic"),
                pair("./r", "require"),
            ]
        );
        assert_eq!(v["imports"][0]["names"], serde_json::json!(["default", "useState"]));
        assert_eq!(v["imports"][4]["names"], serde_json::json!(["x", "z"]));
        assert_eq!(v["exports"], serde_json::json!(["y", "z", "a", "b", "f", "default"]));
    }

    #[test]
    fn analyze_skips_type_only_imports_without_transforming() {
        let v: serde_json::Value = serde_json::from_str(&analyze_json(MODULE, "m.tsx", OPTS)).unwrap();
        let s = specs(&v);
        assert!(!s.contains(&pair("types-only", "import")));
        assert!(s.contains(&pair("./types", "import"))); // not stripped, so still listed
        assert!(s.contains(&pair("react", "import")));
    }

    #[test]
    fn parse_then_print_round_trips() {
        let src = "const a = (x) => x + 1;\nexport default a;";
        let parsed: serde_json::Value = serde_json::from_str(&parse_json(src, "p.js", OPTS)).unwrap();
        let ast = parsed["ast"].to_string();
        assert_eq!(parsed["ast"]["type"], "Module");
        let printed: serde_json::Value = serde_json::from_str(&print_json(&ast, "")).unwrap();
        let code = printed["code"].as_str().unwrap();
        assert!(code.contains("const a = (x)=>x + 1") && code.contains("export default a"), "{code}");
        let min: serde_json::Value =
            serde_json::from_str(&print_json(&ast, r#"{"minify":true}"#)).unwrap();
        assert!(min["code"].as_str().unwrap().len() < code.len());
    }

    #[test]
    fn parse_and_print_report_errors() {
        let v: serde_json::Value = serde_json::from_str(&parse_json("let = ;", "p.js", OPTS)).unwrap();
        assert!(v["error"].is_string());
        let v: serde_json::Value = serde_json::from_str(&print_json("{}", "")).unwrap();
        assert!(v["error"].is_string());
    }

    #[test]
    fn minify_reports_syntax_errors() {
        let out: serde_json::Value = serde_json::from_str(&minify_json("let = ;", "m.js", "")).unwrap();
        assert!(out["error"].is_string());
    }

    #[test]
    fn reports_syntax_errors_with_location() {
        let out: serde_json::Value =
            serde_json::from_str(&transform_json("const a = 1;\nlet b = ;", "src/b.js", OPTS)).unwrap();
        // 1-based line / column of the offending token, prefixed with the file name.
        assert_eq!(out["error"], "src/b.js:2:9: Expression expected");
    }
}


