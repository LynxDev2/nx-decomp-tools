use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::fs;

use crate::functions::{self, demangle_str};

const TYPE_ALIAS_MAP: [(&str, &str, Option<&str>); 19] = [
    ("long", "s64", None),
    ("unsigned long", "u64", None),
    ("int", "s32", None),
    ("unsigned int", "u32", None),
    ("short", "s16", None),
    ("unsigned short", "u16", None),
    ("signed char", "s8", None),
    ("unsigned char", "u8", None),
    ("float", "f32", None),
    ("double", "f64", None),
    ("char16_t", "char16", None),
    (
        "sead::Vector3<float>",
        "sead::Vector3f",
        Some("<math/seadVector.h>"),
    ),
    (
        "sead::Vector3<int>",
        "sead::Vector3i",
        Some("<math/seadVector.h>"),
    ),
    (
        "sead::Vector3<unsigned int>",
        "sead::Vector3u",
        Some("<math/seadVector.h>"),
    ),
    (
        "sead::Matrix22<float>",
        "sead::Matrix22f",
        Some("<math/seadMatrix.h>"),
    ),
    (
        "sead::Matrix33<float>",
        "sead::Matrix33f",
        Some("<math/seadMatrix.h>"),
    ),
    (
        "sead::Matrix34<float>",
        "sead::Matrix34f",
        Some("<math/seadMatrix.h>"),
    ),
    (
        "sead::Matrix44<float>",
        "sead::Matrix44f",
        Some("<math/seadMatrix.h>"),
    ),
    (
        "sead::SafeStringBase<char>",
        "sead::SafeString",
        Some("<prim/seadSafeString.h>"),
    ),
];

// Type symbol -> base classes, vtable functions
pub type TypeInfoMap = HashMap<String, (Vec<String>, Vec<String>)>;

pub fn process_type_info(type_info_map: TypeInfoMap) -> TypeInfoMap {
    type_info_map
        .iter()
        .filter_map(|(class, (base_classes, v))| {
            let class = demangle_str(class).ok()?.to_string();
            let base_classes: Vec<_> = base_classes
                .iter()
                .filter_map(|bc| {
                    Some(
                        demangle_str(bc)
                            .ok()?
                            .strip_prefix("typeinfo for ")?
                            .to_string(),
                    )
                })
                .collect();
            Some((class, (base_classes, v.clone())))
        })
        .collect()
}

pub fn generate_header(
    path: &str,
    functions: &[&functions::Info],
    type_info_map: &TypeInfoMap,
) -> std::io::Result<bool> {
    let mut header = String::new();
    let mut includes = HashSet::new();
    let mut current_namespace = Vec::<NamespaceData>::new();
    let mut fwd_only_namespaces: HashMap<String, HashSet<String>> = HashMap::new();

    for function in functions {
        // Skip base object constructors
        if function.name().contains("C2")
            && functions
                .get(
                    functions
                        .iter()
                        .position(|f| f.name() == function.name())
                        .unwrap()
                        + 1,
                )
                .is_some_and(|f| f.name().contains("C1"))
        {
            continue;
        }

        if function.name().is_empty() {
            header.push_str(&format!(
                "\n// Unhandled unnamed function at: {:#X}",
                function.offset
            ));
        }

        let Ok(demangled) = functions::demangle_str(function.name()) else {
            continue;
        };

        if demangled.contains("thunk(") || demangled.contains("(anonymous namespace)") {
            continue;
        }

        let Some((ident, mut params_str)) = demangled.split_once("(") else {
            continue;
        };
        if ident.contains("<") {
            header.push_str("\n// Unhandled templated function: ");
            header.push_str(&demangled);
            continue;
        }
        let ident_parts: Box<[_]> = ident.split("::").map(String::from).collect();
        params_str = params_str
            .rsplit_once(")")
            .expect("Opening parenthesis should always have a matching closing one")
            .0;
        let mut params: Vec<_> = params_str.split(", ").collect();
        if params[0].is_empty() {
            params.clear();
        }

        if params.is_empty() && !params_str.is_empty() {
            params.push(params_str);
        }

        handle_closing_namespace(&mut header, &mut current_namespace, Some(&ident_parts));

        for (i, part) in ident_parts.iter().enumerate() {
            if i == ident_parts.len() - 1
                || current_namespace.get(i).is_some_and(|n| &n.name == part)
            {
                continue;
            }
            let is_class = !part.starts_with("al") && part.chars().any(|c| c.is_uppercase());
            let base_classes = if is_class {
                type_info_map
                    .get(ident_parts[..i + 1].join("::").as_str())
                    .cloned()
                    .unwrap_or_default()
            } else {
                Default::default()
            }
            .0;
            current_namespace.push(NamespaceData {
                name: part.clone(),
                forward_decls: HashSet::new(),
                functions: Vec::new(),
                lazy_functions: Vec::new(),
                finished_sub_namespaces: Vec::new(),
                is_class,
                base_classes,
            });
        }

        let processed_params: Vec<_> = params
            .into_iter()
            .map(|param| {
                let (mut type_name, mut attributes) = param.rsplit_once(" ").unwrap_or((param, ""));
                let stripped = attributes.strip_prefix("const");
                let mut is_const = false;
                if let Some(other_attributes) = stripped {
                    attributes = other_attributes;
                    is_const = true;
                } else {
                    let last_char = type_name.chars().last().unwrap();
                    if matches!(last_char, '*' | '&') {
                        attributes = &type_name[type_name.len() - 1..];
                        type_name = &type_name[..type_name.len() - 1];
                    }
                }
                if let Some((name, include)) = TYPE_ALIAS_MAP
                    .iter()
                    .find(|(t, _, _)| t == &type_name)
                    .map(|&(_, n, i)| (n, i))
                {
                    type_name = name;
                    if let Some(include) = include {
                        includes.insert(include);
                    }
                } else {
                    let mut latest_removed_namespace = -1;
                    for (i, part) in current_namespace.iter().enumerate() {
                        let Some(namespace_stripped) =
                            type_name.strip_prefix(&format!("{}::", part.name))
                        else {
                            break;
                        };
                        type_name = namespace_stripped;
                        latest_removed_namespace = i as isize;
                    }
                    if latest_removed_namespace >= 0 {
                        current_namespace[latest_removed_namespace as usize]
                            .forward_decls
                            .insert(type_name.to_string());
                    } else if let Some((namespace, name)) = type_name.rsplit_once("::") {
                        fwd_only_namespaces
                            .entry(namespace.to_string())
                            .or_default()
                            .insert(name.to_string());
                    }
                }
                let mut param_type = format!("{type_name}{attributes}");
                if is_const {
                    param_type.insert_str(0, "const ");
                }
                param_type
            })
            .collect();
        let mut fn_end_attribute = if demangled.ends_with("const") {
            String::from(" const")
        } else {
            String::new()
        };

        let fn_name = ident_parts.last().unwrap();

        let return_type = if fn_name.starts_with("~")
            || ident_parts
                .len()
                .checked_sub(2)
                .is_some_and(|i| &ident_parts[i] == fn_name)
        {
            ""
        } else if fn_name.starts_with("is") {
            "bool"
        } else {
            "void"
        };

        if current_namespace.last().is_some_and(|n| n.is_class) {
            if let Some((_, virtual_funcs)) =
                type_info_map.get(ident_parts[..ident_parts.len() - 1].join("::").as_str())
            {
                if virtual_funcs
                    .iter()
                    .find(|vf| vf.as_str() == function.name())
                    .is_some()
                {
                    fn_end_attribute.push_str(" override");
                }
            }
        }
        let mut return_type = return_type.to_string();
        if !return_type.is_empty() {
            return_type.push(' ');
        }

        let fn_text = format!(
            "{return_type}{fn_name}({}){fn_end_attribute};",
            processed_params.join(", ")
        );
        if let Some(namespace_part) = current_namespace.last_mut() {
            if function.lazy {
                namespace_part.lazy_functions.push(fn_text);
            } else {
                namespace_part.functions.push(fn_text);
            }
        } else {
            header.push('\n');
            header.push_str(&fn_text);
        }
    }

    handle_closing_namespace(&mut header, &mut current_namespace, None);

    if includes.is_empty() {
        includes.insert("<basis/seadTypes.h>");
    }

    let mut includes = includes.iter().map(|i| format!("#include {i}"));
    let mut includes_str = includes.join("\n");
    if !includes_str.is_empty() {
        includes_str.push('\n');
    }

    let mut fwd_only_namespaces_str = String::new();

    for (namespace, names) in fwd_only_namespaces.into_iter() {
        fwd_only_namespaces_str.push_str(&format!("namespace {namespace} {{\n"));
        let mut classes = names.iter().map(|n| format!("class {n};"));
        fwd_only_namespaces_str.push_str(&classes.join("\n"));
        fwd_only_namespaces_str.push_str("\n}\n\n");
    }

    if !fwd_only_namespaces_str.is_empty() {
        fwd_only_namespaces_str.insert(0, '\n');
    }

    header.insert_str(
        0,
        &format!("#pragma once\n\n{includes_str}{fwd_only_namespaces_str}"),
    );

    if fs::exists(path)? {
        return Ok(false);
    }

    fs::create_dir_all(path.rsplit_once("/").unwrap_or_default().0)?;
    fs::write(path, header)?;

    Ok(true)
}

fn handle_closing_namespace(
    header: &mut String,
    namespace: &mut Vec<NamespaceData>,
    ident_parts: Option<&[String]>,
) {
    for i in (0..namespace.len()).rev() {
        let part = namespace[i].clone();
        if ident_parts.is_none_or(|parts| parts.get(i).is_none_or(|p| p != &part.name)) {
            if i == 0 {
                header.push('\n');
                header.push_str(&part.to_string());
            } else {
                namespace[i - 1].finished_sub_namespaces.push(part);
            }
            namespace.remove(i);
        }
    }
}

#[derive(Clone, Debug)]
struct NamespaceData {
    name: String,
    forward_decls: HashSet<String>,
    functions: Vec<String>,
    lazy_functions: Vec<String>,
    finished_sub_namespaces: Vec<NamespaceData>,
    is_class: bool,
    base_classes: Vec<String>,
}

impl std::fmt::Display for NamespaceData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Skip namespaces that only have functions from another object
        if self.functions.is_empty() && !self.lazy_functions.is_empty() {
            return Ok(());
        }

        let keyword = if self.is_class { "class" } else { "namespace" };

        let mut base_classes = self.base_classes.iter().map(|b| format!("public {b}"));
        let mut base_classes_str = base_classes.join(", ");

        if !base_classes_str.is_empty() {
            base_classes_str.insert_str(0, " : ");
        }

        writeln!(f, "{keyword} {}{base_classes_str} {{", self.name)?;

        for decl in &self.forward_decls {
            writeln!(f, "class {decl};")?;
        }
        if !self.forward_decls.is_empty() {
            f.write_char('\n')?;
        }

        for namespace in &self.finished_sub_namespaces {
            writeln!(f, "{namespace}\n")?;
        }

        if !self.functions.is_empty() && self.is_class {
            f.write_str("public:\n")?;
        }

        let mut lazy_functions = self
            .lazy_functions
            .iter()
            .map(|lf| format!("{lf} // Function should be implemented in header"));

        let function_section = self.functions.iter().join("\n");

        f.write_str(&function_section)?;

        if !self.lazy_functions.is_empty() {
            f.write_char('\n')?;
            f.write_str(&lazy_functions.join("\n"))?;
        }
        if !self.base_classes.is_empty() {
            let sizeof_str = self
                .base_classes
                .iter()
                .map(|class| format!("sizeof({class})"))
                .join(" - ");
            write!(f, "\nprivate:\ns8 filler[SIZE - {sizeof_str}];\n")?;
        }

        f.write_str("\n}")?;

        if self.is_class {
            f.write_char(';')?;
        }

        f.write_char('\n')?;
        Ok(())
    }
}
