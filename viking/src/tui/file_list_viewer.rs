use std::collections::{HashMap, HashSet};
use std::env;
use std::process::Command;

use anyhow::Result;
use itertools::Itertools;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Style, Stylize};
use ratatui::text::Span;
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use tui_textarea::TextArea;

use crate::functions::demangle_str;
use crate::generate_header::TypeInfoMap;
use crate::tui::TuiWindow;
use crate::{elf, functions, generate_header, repo};

pub struct FileListViewer<'file_list> {
    current_entry_path: String,
    object_map: HashMap<String, &'file_list [functions::Info]>,
    current_entry_items: ViewerItems<'file_list>,
    selected_item: usize,
    search_area: Option<TextArea<'static>>,
    current_search: String,
    decomp_elf: elf::OwnedElf,
    type_info_map: TypeInfoMap,
    filter_out_matching_functions: bool,
}

impl<'file_list> FileListViewer<'file_list> {
    pub fn new(
        file_list: &'file_list functions::FileList,
        decomp_elf: elf::OwnedElf,
        type_info_map: TypeInfoMap,
    ) -> Self {
        let object_map = file_list
            .iter()
            .map(|(path, obj)| {
                (
                    Self::add_possible_prefix_to_object_path(path),
                    &obj.text_section as &'file_list [functions::Info],
                )
            })
            .collect();

        let mut viewer = Self {
            current_entry_path: String::new(),
            object_map,
            current_entry_items: ViewerItems::FileListBrowser(Vec::new()),
            selected_item: 0,
            search_area: None,
            current_search: String::new(),
            decomp_elf,
            type_info_map,
            filter_out_matching_functions: false,
        };
        viewer.update();
        viewer
    }

    fn update(&mut self) {
        if !self.current_entry_path.is_empty() && !self.current_entry_path.ends_with("/") {
            let funcs = self
                .object_map
                .get(&self.current_entry_path)
                .expect("Non folder entries should always have a name that matches an object")
                .iter()
                .filter(|f| {
                    demangle_str(f.name())
                        .unwrap_or_else(|_| f.name().to_string())
                        .contains(&self.current_search)
                        && (!self.filter_out_matching_functions
                            || f.status != functions::Status::Matching)
                })
                .collect();
            self.current_entry_items = ViewerItems::ObjectViewer(funcs);
            return;
        }

        let objects_from_current_path: Vec<&str> = self
            .object_map
            .keys()
            .filter_map(|path| path.strip_prefix(&self.current_entry_path))
            .filter(|name| name.contains(&self.current_search))
            .collect();
        let mut entries_in_current_path: HashSet<&str> = HashSet::new();
        for path in objects_from_current_path {
            let first_component = match path.find("/") {
                Some(index) => &path[..=index],
                None => path,
            };
            entries_in_current_path.insert(first_component);
        }
        let mut entries_sorted = entries_in_current_path.into_iter().collect_vec();
        entries_sorted.sort();
        entries_sorted.insert(0, "..");

        self.current_entry_items = ViewerItems::FileListBrowser(
            entries_sorted
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let (kind, matching_percentage) = if *e == ".." {
                        (ViewerEntryKind::FolderUp, None)
                    } else if e.ends_with("/") {
                        let (folder_total_functions, folder_matching_functions) = self
                            .object_map
                            .iter()
                            .filter(|(path, _)| {
                                path.starts_with(&format!("{}{e}", &self.current_entry_path))
                            })
                            .fold((0usize, 0usize), |(total, matching), (_, funcs)| {
                                let matching_count = funcs
                                    .iter()
                                    .filter(|f| f.status == functions::Status::Matching)
                                    .count();
                                (total + funcs.len(), matching + matching_count)
                            });
                        let matching_percentage = ((folder_matching_functions as f32
                            / folder_total_functions as f32)
                            * 100.0)
                            .floor() as u8;
                        (ViewerEntryKind::Folder, Some(matching_percentage))
                    } else {
                        let funcs = self
                        .object_map
                        .get(&format!("{}{e}", &self.current_entry_path))
                        .expect(
                            "Non folder entries should always have a name that matches an object",
                        );
                        let matching_count = funcs
                            .iter()
                            .filter(|f| f.status == functions::Status::Matching)
                            .count();
                        let matching_percentage =
                            ((matching_count as f32 / funcs.len() as f32) * 100.0).floor() as u8;
                        (ViewerEntryKind::Object, Some(matching_percentage))
                    };
                    ViewerEntry {
                        is_selected: i == self.selected_item,
                        name: e.to_string(),
                        kind,
                        matching_percentage,
                    }
                })
                .collect(),
        );
    }

    fn add_possible_prefix_to_object_path(path: &str) -> String {
        let config = repo::get_config();
        if path.contains("Unknown/") || !path.contains("/") {
            return format!("misc/{path}");
        }
        if config
            .file_list_root_folders
            .as_ref()
            .is_some_and(|ps| ps.iter().any(|p| path.starts_with(p)))
        {
            return path.to_string();
        }
        format!("src/{path}")
    }

    fn open_editor_at(path: &str, line: u32) -> Result<()> {
        let editor = env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());

        if editor == "code" {
            // VS Code
            Command::new(&editor)
                .arg("--goto")
                .arg(format!("{}:{}", path, line))
                .status()?;
        } else if editor == "subl" {
            // Sublime Text
            Command::new(&editor)
                .arg(format!("{}:{}", path, line))
                .status()?;
        } else if editor == "vim" || editor == "nvim" || editor == "vi" || editor == "nano" {
            // Vim, Neovim, Nano, etc.
            Command::new(&editor)
                .arg(format!("+{}", line))
                .arg(path)
                .status()?;
        } else {
            // Default fallback: just open the path
            Command::new(&editor).arg(path).status()?;
        }
        Ok(())
    }
}

impl TuiWindow for FileListViewer<'_> {
    fn draw(&self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .split(frame.area());

        let mut state = ListState::default();
        state.select(Some(self.selected_item));

        let item_count;
        let list = match &self.current_entry_items {
            ViewerItems::FileListBrowser(items) => {
                item_count = items.len() - 1; // -1 for ignoring ".."
                List::new(items.iter().cloned().map(Into::<ListItem>::into))
            }
            ViewerItems::ObjectViewer(infos) => {
                let mut items = infos
                    .iter()
                    .enumerate()
                    .map(|(i, info)| {
                        ListItem::new(Span::styled(
                            info.to_string(),
                            info.status.to_list_item_style(self.selected_item == i + 1),
                        ))
                    })
                    .collect_vec();
                let exit_view_style = if self.selected_item == 0 {
                    Style::new().on_blue().black().bold()
                } else {
                    Style::new().blue()
                };
                item_count = items.len();
                items.insert(0, ListItem::new(Span::styled("..", exit_view_style)));
                List::new(items)
            }
        };
        frame.render_stateful_widget(list, chunks[0], &mut state);

        if let Some(ref search_area) = self.search_area {
            frame.render_widget(search_area, chunks[1]);
        }

        let mut footer_text = String::from("↑↓ navigate • q quit");

        if matches!(self.current_entry_items, ViewerItems::ObjectViewer(_)) {
            footer_text.push_str(
                "• Enter view asm diff • O open function in editor • M toggle don't show matching",
            );
        }

        if !self.current_search.is_empty() {
            footer_text.push_str(" • Esc clear search");
        }

        footer_text.push_str(&format!("\t\t\t ({item_count} items)"));

        frame.render_widget(Paragraph::new(footer_text), chunks[2]);
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if let Some(mut search_area) = self.search_area.take() {
            if key.code == KeyCode::Enter {
                search_area.lines()[0].clone_into(&mut self.current_search);
                self.selected_item = 0;
                self.update();
            } else if key.code != KeyCode::Esc {
                search_area.input(key);
                self.search_area = Some(search_area);
            }
            return false;
        }
        match key.code {
            KeyCode::Char('/') => self.search_area = Some(TextArea::default()),
            KeyCode::Down => {
                let items_len = match &self.current_entry_items {
                    ViewerItems::FileListBrowser(items) => items.len(),
                    // Account for extra entry for ".."
                    ViewerItems::ObjectViewer(infos) => infos.len() + 1,
                };
                if self.selected_item < items_len - 1 {
                    self.selected_item += 1;
                }
            }
            KeyCode::Char('q') => return true,
            KeyCode::Up => {
                if self.selected_item > 0 {
                    self.selected_item -= 1;
                }
            }
            KeyCode::Esc => {
                self.current_search.clear();
                self.update();
            }
            KeyCode::Char('M') => {
                self.filter_out_matching_functions = !self.filter_out_matching_functions;
                self.update();
            }
            KeyCode::Char('H') => {
                if let ViewerItems::ObjectViewer(infos) = &self.current_entry_items {
                    let mut path = self.current_entry_path.replace(".o", ".h");
                    // TODO: Don't hardcode this
                    if path.starts_with("Library/") || path.starts_with("Project/") {
                        path.insert_str(0, "lib/al/");
                    }
                    let _ = generate_header::generate_header(&path, &infos, &self.type_info_map);
                }
            }
            KeyCode::Char('O') => {
                if let ViewerItems::ObjectViewer(infos) = &self.current_entry_items {
                    if self.selected_item == 0 {
                        return false;
                    }
                    let Ok(ctx) = elf::create_addr2line_ctx_for(&self.decomp_elf) else {
                        return false;
                    };
                    let Ok((file_path, line_num)) = elf::find_file_and_line_by_symbol(
                        &self.decomp_elf,
                        &ctx,
                        infos[self.selected_item - 1].name(),
                    ) else {
                        return false;
                    };
                    return Self::open_editor_at(&file_path, line_num).is_ok();
                }
            }

            KeyCode::Enter => {
                match &self.current_entry_items {
                    ViewerItems::FileListBrowser(items) => {
                        let item = &items[self.selected_item];
                        match item.kind {
                            ViewerEntryKind::FolderUp => {
                                if !self.current_entry_path.is_empty() {
                                    if let Some(index) = self.current_entry_path
                                        [..self.current_entry_path.len() - 1]
                                        .rfind("/")
                                    {
                                        self.current_entry_path.truncate(index + 1);
                                    } else {
                                        self.current_entry_path.clear();
                                    }
                                }
                            }
                            ViewerEntryKind::Folder | ViewerEntryKind::Object => {
                                self.current_entry_path.push_str(&item.name)
                            }
                        }
                        self.selected_item = 0;
                        self.current_search.clear();
                    }
                    ViewerItems::ObjectViewer(infos) => {
                        if self.selected_item == 0 {
                            // ".."
                            self.current_entry_path.truncate(
                                self.current_entry_path
                                    .rfind("/")
                                    .expect("Objects should always be in a folder")
                                    + 1,
                            );
                        } else {
                            infos[self.selected_item - 1]
                                .show_asm_differ_for(None, &[], None)
                                .expect("Failed to show asm-differ for function");
                        }
                    }
                };
            }

            _ => {}
        };
        self.update();
        false
    }
}

#[derive(Clone, Debug)]
enum ViewerItems<'a> {
    FileListBrowser(Vec<ViewerEntry>),
    ObjectViewer(Box<[&'a functions::Info]>),
}

#[derive(Clone, Debug)]
struct ViewerEntry {
    is_selected: bool,
    name: String,
    kind: ViewerEntryKind,
    matching_percentage: Option<u8>,
}

#[derive(Clone, Copy, Debug)]
enum ViewerEntryKind {
    FolderUp,
    Folder,
    Object,
}

impl From<ViewerEntry> for ListItem<'static> {
    fn from(val: ViewerEntry) -> Self {
        let text = match val.matching_percentage {
            Some(percentage) => format!("{} ({}%)", val.name, percentage),
            None => val.name,
        };
        let style = match val.kind {
            ViewerEntryKind::FolderUp | ViewerEntryKind::Folder => {
                if val.is_selected {
                    Style::new().on_blue().black().bold()
                } else {
                    Style::new().blue()
                }
            }
            ViewerEntryKind::Object => match (
                val.matching_percentage
                    .expect("Objects should always have a matching perecentage"),
                val.is_selected,
            ) {
                (0, false) => Style::new().red(),
                (0, true) => Style::new().black().bold().on_red(),
                (1..100, false) => Style::new().yellow(),
                (1..100, true) => Style::new().black().bold().on_yellow(),
                (100, false) => Style::new().green(),
                (100, true) => Style::new().black().bold().on_green(),
                _ => Style::new(),
            },
        };
        ListItem::new(Span::styled(text, style))
    }
}
