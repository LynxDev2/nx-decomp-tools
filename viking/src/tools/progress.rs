use std::collections::HashMap;

use anyhow::bail;
use viking::{
    functions::{self, FileList, Status},
    repo,
};

use clap::Parser;
use colored::Colorize;
use enum_map::EnumMap;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    no_namespace_progress: bool,
    #[arg(
        long,
        short,
        default_value = "",
        default_missing_value = "HEAD",
        num_args = 0..=1
    )]
    compare: String,
    #[arg(long)]
    object_changes_only: bool,
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
    let mut completed_objects_str = format!(
        "{}/{}",
        current_stats
            .remaining_functions
            .values()
            .filter(|&&c| c == 0)
            .count(),
        current_stats.object_count
    );

    if let Some(rev_stats) = git_rev_stats {
        completed_objects_str.insert_str(
            0,
            &format!(
                " -> {}/{}",
                rev_stats
                    .remaining_functions
                    .values()
                    .filter(|&&c| c == 0)
                    .count(),
                rev_stats.object_count
            ),
        );
    }

    println!(
        "  {}: {completed_objects_str}\n",
        "Completed objects".bright_blue(),
    );

    for (status, current_count) in current_stats.function_statuses {
        let currrent_code_size = current_stats.code_size[status];
        let mut count_str = format!("{current_count:>7}");
        let mut count_percentage_str = make_three_decimal_percentage(current_count, current_stats.total_function_count);
        let mut size_percentage_str = make_three_decimal_percentage(currrent_code_size, current_stats.total_code_size);

        if let Some(rev_stats) = git_rev_stats {
            let rev_code_size = rev_stats.code_size[status];
            let rev_count = rev_stats.function_statuses[status];

            count_str.insert_str(0, &format!("{rev_count:>7} -> "));
            count_percentage_str.insert_str(
                0,
                &format!(
                    "{} -> ",
                    make_three_decimal_percentage(rev_count, rev_stats.total_function_count)
                ),
            );
            size_percentage_str.insert_str(
                0,
                &format!(
                    "{} -> ",
                    make_three_decimal_percentage(rev_code_size, rev_stats.total_code_size)
                ),
            );
        }

        println!(
            "{count_str} {} ({count_percentage_str} | size: {size_percentage_str})",
            status.description().color(status.color()),
        );
    }
}

fn print_changed_objects(current_stats: &ProgressStats, git_rev_stats: &ProgressStats) {
    for (object_name, remaining_functions) in &current_stats.remaining_functions {
        if git_rev_stats
            .remaining_functions
            .get(object_name)
            .is_none_or(|r| remaining_functions < r)
        {
            println!("{object_name}");
        }
    }
}

fn print_raw_matching_data(stats: &ProgressStats) {
    println!(
        "matching={}",
        make_three_decimal_percentage(
            stats.function_statuses[Status::Matching],
            stats.total_function_count
        )
    );
    println!(
        "minor={}",
        make_three_decimal_percentage(
            stats.function_statuses[Status::NonMatchingMinor],
            stats.total_function_count
        )
    );
    println!(
        "major={}",
        make_three_decimal_percentage(
            stats.function_statuses[Status::NonMatchingMajor],
            stats.total_function_count
        )
    );
}

fn print_namespace_progress(main_namespaces: &[NamespaceStat]) {
    println!("\nNamespace progress:");

    for namespace_stat in main_namespaces {
        println!(
            "{:>7} {} ({})",
            namespace_stat.decompiled_functions,
            namespace_stat.name.cyan(),
            make_three_decimal_percentage(
                namespace_stat.decompiled_functions,
                namespace_stat.total_functions
            ),
        );
    }
}

struct NamespaceStat {
    name: String,
    total_functions: usize,
    decompiled_functions: usize,
}

#[derive(Default)]
struct ProgressStats {
    remaining_functions: HashMap<String, usize>,
    function_statuses: EnumMap<functions::Status, usize>,
    code_size: EnumMap<functions::Status, usize>,
    total_function_count: usize,
    total_code_size: usize,
    main_namespaces: Vec<NamespaceStat>,
    object_count: usize,
}

fn calc_file_list_stats(file_list: &FileList) -> ProgressStats {
    let mut stats = ProgressStats {
        object_count: file_list.len(),
        ..Default::default()
    };
    let mut namespaces: HashMap<String, (usize, usize)> = HashMap::new();
    for (object_name, object) in file_list {
        for function in &object.text_section {
            stats.function_statuses[function.status] += 1;
            stats.code_size[function.status] += function.size as usize;

            stats.total_function_count += 1;
            stats.total_code_size += function.size as usize;

            if let Some(mut root_namespace) = functions::demangle_str(function.name())
                .ok()
                .and_then(|n| n.split_once("::").map(|(a, _)| a.to_string()))
            {
                if root_namespace
                    .chars()
                    .next()
                    .unwrap_or_default()
                    .is_uppercase()
                {
                    root_namespace = String::from("Global namespace (game)");
                }
                if root_namespace != "std" {
                    let namespace_info = namespaces.entry(root_namespace).or_default();
                    namespace_info.1 += 1;
                    if function.is_decompiled() {
                        namespace_info.0 += 1;
                    }
                }
            }
        }
        let remaining_functions = object
            .text_section
            .iter()
            .filter(|func| !func.is_decompiled())
            .count();
        if remaining_functions != object.text_section.len() {
            stats
                .remaining_functions
                .insert(object_name.clone(), remaining_functions);
        }
    }

    stats.main_namespaces = namespaces
        .into_iter()
        .filter(|(_, (_, total_functions))| *total_functions > 500)
        .map(
            |(name, (decompiled_functions, total_functions))| NamespaceStat {
                name,
                total_functions,
                decompiled_functions,
            },
        )
        .collect();

    stats
        .main_namespaces
        .sort_unstable_by_key(|b| std::cmp::Reverse(b.total_functions));

    stats
}

fn make_three_decimal_percentage(num: usize, total: usize) -> String {
    let percentage = num as f32 / total as f32 * 100.0;
    format!("{percentage:.3}%")
}
