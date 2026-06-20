use std::collections::HashMap;

use anyhow::bail;
use viking::{
    functions::{self, FileList, Status},
    repo,
};

use clap::Parser;
use colored::Colorize;
use enum_map::EnumMap;

/// Print the current status or progress of a decomp project
#[derive(Parser)]
struct Args {
    /// Don't show namespace progress
    #[arg(long)]
    no_namespace_progress: bool,
    /// Compare progress to a git rev (defaults to HEAD when used)
    #[arg(
        long,
        short,
        default_value = "",
        default_missing_value = "HEAD",
        num_args = 0..=1
    )]
    compare: String,
    /// Only list what objects have changed (requires also using --compare)
    #[arg(long)]
    object_changes_only: bool,
    /// Only show raw status data (STATUS=PERCENTAGE)
    #[arg(long)]
    raw_status_data_only: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    if args.object_changes_only && args.compare.is_empty() {
        bail!("--compare must be specified when using --object-changes-only");
    }

    let current_file_list =
        functions::parse_file_list(functions::get_file_list_path(None).as_path())?;
    let current_stats = calc_file_list_stats(&current_file_list);

    if args.raw_status_data_only {
        print_raw_matching_data(&current_stats);
        return Ok(());
    }

    let git_rev_stats = if !args.compare.is_empty() {
        let rev_file_list_content =
            repo::get_file_contents_at_git_rev(&args.compare, &repo::get_config().file_list)?;
        let rev_file_list = functions::parse_file_list_from_str(&rev_file_list_content)?;
        Some(calc_file_list_stats(&rev_file_list))
    } else {
        None
    };

    if args.object_changes_only {
        print_changed_objects(&current_stats, git_rev_stats.as_ref().unwrap());
        return Ok(());
    }

    print_current_progress(&current_stats, git_rev_stats.as_ref());

    if !args.no_namespace_progress {
        print_namespace_progress(&current_stats.main_namespaces);
    }

    Ok(())
}

fn print_current_progress(current_stats: &ProgressStats, git_rev_stats: Option<&ProgressStats>) {
    let completed = |s: &ProgressStats| {
        s.incomplete_remaining_functions
            .values()
            .filter(|&&c| c == 0)
            .count()
    };
    let cur_completed = completed(current_stats);

    let completed_objects_str = if let Some(rev) = git_rev_stats {
        cmp_fmt(
            format!("{}/{}", completed(rev), rev.object_count),
            format!("{cur_completed}/{}", current_stats.object_count),
        )
    } else {
        format!("{cur_completed}/{}", current_stats.object_count)
    };

    println!(
        "  {}: {completed_objects_str}\n",
        "Completed objects".bright_blue(),
    );

    for (status, current_count) in current_stats.function_statuses {
        let current_code_size = current_stats.code_size[status];

        let (count_str, count_pct_str, size_pct_str) = if let Some(rev) = git_rev_stats {
            let rev_count = rev.function_statuses[status];
            let rev_code_size = rev.code_size[status];
            (
                cmp_fmt(format!("{rev_count:>7}"), format!("{current_count:>7}")),
                cmp_fmt(
                    pct(rev_count, rev.total_function_count),
                    pct(current_count, current_stats.total_function_count),
                ),
                cmp_fmt(
                    pct(rev_code_size, rev.total_code_size),
                    pct(current_code_size, current_stats.total_code_size),
                ),
            )
        } else {
            (
                format!("{current_count:>7}"),
                pct(current_count, current_stats.total_function_count),
                pct(current_code_size, current_stats.total_code_size),
            )
        };

        println!(
            "{count_str} {} ({count_pct_str} | size: {size_pct_str})",
            status.description().color(status.color()),
        );
    }
}

fn print_changed_objects(current_stats: &ProgressStats, git_rev_stats: &ProgressStats) {
    for (object_name, remaining_functions) in &current_stats.incomplete_remaining_functions {
        if git_rev_stats
            .incomplete_remaining_functions
            .get(object_name)
            .is_none_or(|r| remaining_functions < r)
        {
            println!("{object_name}");
        }
    }
}

fn print_raw_matching_data(stats: &ProgressStats) {
    for (label, status) in [
        ("matching", Status::Matching),
        ("minor", Status::NonMatchingMinor),
        ("major", Status::NonMatchingMajor),
    ] {
        println!(
            "{label}={}",
            pct(stats.function_statuses[status], stats.total_function_count)
        );
    }
}

fn print_namespace_progress(main_namespaces: &[(String, NamespaceStat)]) {
    println!("\nNamespace progress:");

    for (name, namespace_stat) in main_namespaces {
        println!(
            "{:>7} {} ({})",
            namespace_stat.decompiled_functions,
            name.cyan(),
            pct(
                namespace_stat.decompiled_functions,
                namespace_stat.total_functions
            ),
        );
    }
}

#[derive(Default)]
struct NamespaceStat {
    total_functions: usize,
    decompiled_functions: usize,
}

#[derive(Default)]
struct ProgressStats {
    /// Stores the remaining functions for every incomplete but not fully unimplemented object
    incomplete_remaining_functions: HashMap<String, usize>,
    function_statuses: EnumMap<functions::Status, usize>,
    code_size: EnumMap<functions::Status, usize>,
    total_function_count: usize,
    total_code_size: usize,
    main_namespaces: Vec<(String, NamespaceStat)>,
    object_count: usize,
}

fn calc_file_list_stats(file_list: &FileList) -> ProgressStats {
    let mut stats = ProgressStats {
        object_count: file_list.len(),
        ..Default::default()
    };
    let mut namespaces: HashMap<String, NamespaceStat> = HashMap::new();
    for (object_name, object) in file_list {
        for function in &object.text_section {
            stats.function_statuses[function.status] += 1;
            stats.code_size[function.status] += function.size as usize;

            stats.total_function_count += 1;
            stats.total_code_size += function.size as usize;

            accumulate_namespace(function, &mut namespaces);
        }
        let remaining_functions = object
            .text_section
            .iter()
            .filter(|func| !func.is_decompiled())
            .count();
        if remaining_functions != object.text_section.len() {
            stats
                .incomplete_remaining_functions
                .insert(object_name.clone(), remaining_functions);
        }
    }

    const MAIN_NAMESPACE_MIN_FUNCTIONS: usize = 500;

    stats.main_namespaces = namespaces
        .into_iter()
        .filter(|(_, stats)| stats.total_functions > MAIN_NAMESPACE_MIN_FUNCTIONS)
        .collect();
    stats
        .main_namespaces
        .sort_unstable_by_key(|(_, stats)| std::cmp::Reverse(stats.total_functions));

    stats
}

fn pct(num: usize, total: usize) -> String {
    let percentage = num as f32 / total as f32 * 100.0;
    format!("{percentage:.3}%")
}

fn cmp_fmt(rev: impl std::fmt::Display, cur: impl std::fmt::Display) -> String {
    format!("{rev} -> {cur}")
}

fn accumulate_namespace(func: &functions::Info, namespaces: &mut HashMap<String, NamespaceStat>) {
    if let Some(root_namespace) = functions::demangle_str(func.name())
        .ok()
        .and_then(|n| n.split_once("::").map(|(a, _)| a.to_string()))
    {
        let root_namespace = if root_namespace.starts_with(char::is_uppercase) {
            "Global namespace (game)".into()
        } else {
            root_namespace
        };
        if root_namespace != "std" {
            let stats = namespaces.entry(root_namespace).or_default();
            stats.total_functions += 1;
            if func.is_decompiled() {
                stats.decompiled_functions += 1;
            }
        }
    }
}
