use rkyv::rancor;
use viking::functions::FileList;
use viking::generate_header::TypeInfoMap;
use viking::{tui::TuiWindow, *};

use anyhow::{bail, Result};
use ratatui::crossterm::event::{self, Event};
use ratatui::DefaultTerminal;
use std::time::Duration;
use std::{env, fs};

fn print_help(bin_name: &str) {
    println!("Usage: {bin_name} [help/view/edit/generate-header /path/to/object.o] [--typeinfo /path/to/typeinfo.dat]

help - show this message
view - open the file list viewer TUI
edit - open the file list editor TUI
generate-header - generate a header for the given object")
}

fn try_get_typeinfo(args: &[String]) -> TypeInfoMap {
    let path = if args.len() > 2 && args[args.len() - 2] == "--typeinfo" {
        args[args.len() - 1].as_str()
    } else {
        "typeinfo.dat"
    };

    let type_info = fs::read(path)
        .ok()
        .and_then(|file| rkyv::from_bytes::<_, rancor::Error>(file.as_slice()).ok());

    type_info
        .map(generate_header::process_type_info)
        .unwrap_or_else(|| {
            eprintln!("Warning: Could not parse typeinfo, header generation will be limited");
            TypeInfoMap::new()
        })
}

fn generate_header(object_path: &str, file_list: &FileList, type_info: &TypeInfoMap) -> bool {
    let Some((_, object)) = file_list.iter().find(|(path, _)| *path == object_path) else {
        eprint!("Failed to create header: Object {object_path} not found in file list");
        return false;
    };

    let mut header_path = object_path.replace(".o", ".h");
    // TODO: Don't hardcode this
    if object_path.starts_with("Library/") || object_path.starts_with("Project/") {
        header_path.insert_str(0, "lib/al/");
    } else {
        header_path.insert_str(0, "src/");
    }

    let functions: Box<_> = object.text_section.iter().collect();
    let res = generate_header::generate_header(&header_path, &functions, type_info);

    match res {
        Ok(success) if !success => {
            eprintln!("Failed to create header: File already exists");
            return false;
        }
        Err(e) => {
            eprint!("Failed to create header: {e}");
            return false;
        }
        _ => {}
    }

    true
}

fn main() -> Result<()> {
    let cli_args: Vec<String> = env::args().collect();
    let bin_name = cli_args[0].clone();
    let args = &cli_args[1..];

    let Some(cmd) = args.first() else {
        print_help(&bin_name);
        return Ok(());
    };

    let mut file_list = functions::parse_file_list(functions::get_file_list_path(None).as_path())?;

    let mut window: Box<dyn TuiWindow> = match cmd.as_str() {
        "viewer" => {
            let decomp_elf = elf::load_decomp_elf(None)?;
            Box::new(tui::file_list_viewer::FileListViewer::new(
                &file_list,
                decomp_elf,
                try_get_typeinfo(args),
            ))
        }
        "editor" => Box::new(tui::file_list_editor::FileListEditor::new(&mut file_list)),
        "generate-header" => {
            let Some(path) = args.get(1) else {
                bail!("No object specified")
            };
            let type_info = try_get_typeinfo(args);
            if !generate_header(path, &file_list, &type_info) {
                std::process::exit(1);
            } else {
                return Ok(());
            }
        }
        "help" => {
            print_help(&bin_name);
            return Ok(());
        }
        _ => {
            print_help(&bin_name);
            bail!("Invalid subcommand");
        }
    };

    let mut terminal = ratatui::init();
    terminal.clear()?;
    let res = loop {
        let should_exit = main_loop(&mut terminal, window.as_mut())?;
        if should_exit {
            break Ok(());
        }
    };
    ratatui::restore();
    res
}

fn main_loop(terminal: &mut DefaultTerminal, window: &mut dyn TuiWindow) -> Result<bool> {
    terminal.draw(|f| window.draw(f))?;

    if event::poll(Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            if window.handle_key(key) {
                return Ok(true);
            }
        }
    }

    Ok(false)
}
