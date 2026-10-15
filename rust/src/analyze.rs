//! Collects a module's static dependency graph (imports / re-exports / `require` / `import()`)
//! and its export names, without executing it.
use std::{cell::RefCell, rc::Rc};

use serde::Serialize;
use swc_core::ecma::ast::*;
use swc_core::ecma::utils::find_pat_ids;
use swc_core::ecma::visit::{noop_visit_type, Visit, VisitWith};

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Dependency {
    pub specifier: String,
    /// `import` | `export-from` | `export-all` | `dynamic` | `require`
    pub kind: &'static str,
    /// Imported / re-exported binding names (`default`, `*` for a namespace import); empty for
    /// side-effect imports, dynamic imports and requires.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
}

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Analysis {
    pub imports: Vec<Dependency>,
    /// Exported binding names (`default` included); `export * from` is only listed in `imports`.
    pub exports: Vec<String>,
}

pub fn analyze<N: VisitWith<Collector>>(node: &N) -> Analysis {
    let sink = Rc::new(RefCell::new(Analysis::default()));
    node.visit_with(&mut Collector::new(sink.clone()));
    sink.take()
}

/// Visitor that appends to a shared `Analysis`, so it can also run as a pass inside the SWC
/// pipeline (`visit_pass(Collector::new(sink))`) and be read after the pipeline finished.
pub struct Collector {
    out: Rc<RefCell<Analysis>>,
}

fn export_name(n: &ModuleExportName) -> String {
    match n {
        ModuleExportName::Ident(i) => i.sym.to_string(),
        ModuleExportName::Str(s) => s.value.to_string_lossy().into_owned(),
    }
}

impl Collector {
    pub fn new(out: Rc<RefCell<Analysis>>) -> Self {
        Self { out }
    }

    fn dep(&mut self, specifier: String, kind: &'static str, names: Vec<String>) {
        self.out.borrow_mut().imports.push(Dependency { specifier, kind, names });
    }

    fn export(&mut self, name: String) {
        let mut out = self.out.borrow_mut();
        if !out.exports.contains(&name) {
            out.exports.push(name);
        }
    }
}

impl Visit for Collector {
    noop_visit_type!();

    fn visit_import_decl(&mut self, n: &ImportDecl) {
        if n.type_only {
            return;
        }
        let mut names = Vec::new();
        for s in &n.specifiers {
            match s {
                ImportSpecifier::Named(s) if !s.is_type_only => names.push(
                    s.imported.as_ref().map(export_name).unwrap_or_else(|| s.local.sym.to_string()),
                ),
                ImportSpecifier::Default(_) => names.push("default".into()),
                ImportSpecifier::Namespace(_) => names.push("*".into()),
                _ => {}
            }
        }
        self.dep(n.src.value.to_string_lossy().into_owned(), "import", names);
    }

    fn visit_export_all(&mut self, n: &ExportAll) {
        if !n.type_only {
            self.dep(n.src.value.to_string_lossy().into_owned(), "export-all", vec!["*".into()]);
        }
    }

    fn visit_named_export(&mut self, n: &NamedExport) {
        if n.type_only {
            return;
        }
        let mut from = Vec::new();
        for s in &n.specifiers {
            match s {
                ExportSpecifier::Named(s) if !s.is_type_only => {
                    let orig = export_name(&s.orig);
                    let exported = s.exported.as_ref().map(export_name).unwrap_or_else(|| orig.clone());
                    self.export(exported);
                    from.push(orig);
                }
                ExportSpecifier::Default(s) => {
                    self.export(s.exported.sym.to_string());
                    from.push("default".into());
                }
                ExportSpecifier::Namespace(s) => {
                    self.export(export_name(&s.name));
                    from.push("*".into());
                }
                _ => {}
            }
        }
        if let Some(src) = &n.src {
            self.dep(src.value.to_string_lossy().into_owned(), "export-from", from);
        }
    }

    fn visit_export_decl(&mut self, n: &ExportDecl) {
        match &n.decl {
            Decl::Class(c) => self.export(c.ident.sym.to_string()),
            Decl::Fn(f) => self.export(f.ident.sym.to_string()),
            Decl::Var(v) => {
                for id in find_pat_ids::<_, Id>(&v.decls) {
                    self.export(id.0.to_string());
                }
            }
            _ => {}
        }
        n.decl.visit_children_with(self);
    }

    fn visit_export_default_decl(&mut self, n: &ExportDefaultDecl) {
        self.export("default".into());
        n.decl.visit_children_with(self);
    }

    fn visit_export_default_expr(&mut self, n: &ExportDefaultExpr) {
        self.export("default".into());
        n.expr.visit_with(self);
    }

    fn visit_ts_external_module_ref(&mut self, n: &TsExternalModuleRef) {
        self.dep(n.expr.value.to_string_lossy().into_owned(), "require", vec![]);
    }

    fn visit_call_expr(&mut self, n: &CallExpr) {
        let first = n.args.first().filter(|a| a.spread.is_none());
        let literal = first.and_then(|a| match &*a.expr {
            Expr::Lit(Lit::Str(s)) => Some(s.value.to_string_lossy().into_owned()),
            Expr::Tpl(t) if t.exprs.is_empty() && t.quasis.len() == 1 => {
                t.quasis[0].cooked.as_ref().map(|c| c.to_string_lossy().into_owned())
            }
            _ => None,
        });
        match (&n.callee, literal) {
            (Callee::Import(_), Some(s)) => self.dep(s, "dynamic", vec![]),
            (Callee::Expr(e), Some(s)) if matches!(&**e, Expr::Ident(i) if &*i.sym == "require") => {
                self.dep(s, "require", vec![])
            }
            _ => {}
        }
        n.visit_children_with(self);
    }
}
