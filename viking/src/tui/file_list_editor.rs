use std::collections::HashSet;

use ratatui::crossterm::event::KeyCode;
use ratatui::text::Line;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Style, Stylize},
    text::Span,
    widgets::{List, ListItem, ListState, Paragraph},
};
use tui_textarea::TextArea;

use crate::{functions, tui::TuiWindow};

pub struct FileListEditor<'a> {
    items: Vec<EditorEntry>,
    file_list: &'a mut functions::FileList,
    expanded_objects: HashSet<String>,
    selected_item: usize,
    text_input: Option<(TextArea<'static>, TextInputType, String, &'static str)>,
}

impl<'a> FileListEditor<'a> {
    pub fn new(file_list: &'a mut functions::FileList) -> Self {
        let mut editor = FileListEditor {
            items: Vec::with_capacity(file_list.len()),
            file_list,
            expanded_objects: HashSet::new(),
            selected_item: 0,
            text_input: None,
        };
        editor.update();
        editor
    }

    fn update(&mut self) {
        self.items.clear();
        for (name, object) in self.file_list.iter() {
            let is_expanded = self.expanded_objects.contains(name.as_str());
            self.items.push(EditorEntry::new(EditorEntryData::Object(
                name.clone(),
                is_expanded,
            )));
            if is_expanded {
                self.items.extend(
                    object
                        .text_section
                        .iter()
                        .map(|f| EditorEntry::new(EditorEntryData::Function(f.clone()))),
                );
            }
        }
        if self.selected_item < self.items.len() {
            self.items[self.selected_item].is_selected = true;
        } else {
            self.update_selected(self.items.len());
        }
    }

    fn update_selected(&mut self, selected: usize) {
        self.items[self.selected_item].is_selected = false;
        self.items[selected].is_selected = true;
        self.selected_item = selected;
    }

    fn add_split_at_current_item(&mut self, checked_name: &str) {
        let EditorEntryData::Function(selected_function) =
            self.items[self.selected_item].data.clone()
        else {
            panic!("Selected item needs to be of type Function");
        };
        let object_name = self.items[..self.selected_item]
            .iter()
            .rev()
            .find_map(|i| {
                if let EditorEntryData::Object(name, _) = &i.data {
                    Some(name)
                } else {
                    None
                }
            })
            .expect("there should always be an object before the current item");
        let object_index = self
            .file_list
            .iter()
            .position(|(obj_name, _)| obj_name == object_name)
            .expect("objects that exist in the viewer should also exist in the file list");
        let object = &mut self.file_list[object_index].1;
        let cur_func_pos = object
            .text_section
            .iter()
            .position(|f| f.name() == selected_function.name())
            .expect("The selected function should always exist in the file list");
        let funcs = object.text_section.drain(cur_func_pos..).collect();
        let new_obj = functions::Object {
            text_section: funcs,
        };
        self.file_list
            .insert(object_index + 1, (checked_name.to_string(), new_obj));
        self.expanded_objects.insert(checked_name.to_string());
        self.update();
    }

    fn remove_selected_split(&mut self) {
        let EditorEntryData::Object(ref object_name, _) =
            self.items[self.selected_item].data.clone()
        else {
            panic!("Selected item needs to be of type Object");
        };
        let object_index = self
            .file_list
            .iter()
            .position(|(obj_name, _)| obj_name == object_name)
            .expect("objects that exist in the viewer should also exist in the file list");
        if object_index == 0 {
            return;
        }
        let object = self.file_list.remove(object_index).1;
        self.file_list[object_index - 1]
            .1
            .text_section
            .extend(object.text_section);
        self.update();
    }

    fn save_file_list(&self) {
        functions::write_functions_to_path(
            &functions::get_file_list_path(None).as_path(),
            self.file_list.clone(),
        )
        .expect("Failed to save file list");
    }
}

impl TuiWindow for FileListEditor<'_> {
    fn draw(&self, frame: &mut ratatui::Frame) {
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

        let list = List::new(
            self.items
                .iter()
                .map(|i| i.to_list_item(frame.area().width as i32)),
        );
        frame.render_stateful_widget(list, chunks[0], &mut state);

        let mut info_text = match &self.items[self.selected_item].data {
            EditorEntryData::Object(_, _) => "↵ expand/collapse • ↑↓ navigate • / search for split • D delete split • R rename split • S save • Q quit",
            EditorEntryData::Function(_) => "↑↓ navigate • / search for split • C create split • S save • Q quit",
        };

        if let Some((ref text_area, _, _, desc)) = self.text_input {
            info_text = desc;
            frame.render_widget(text_area, chunks[1]);
        }

        frame.render_widget(Paragraph::new(info_text), chunks[2]);
    }

    fn handle_key(&mut self, key: ratatui::crossterm::event::KeyEvent) -> bool {
        use TextInputType::*;
        if let Some((mut text_input, input_type, previous_input, desc)) = self.text_input.take() {
            if key.code == KeyCode::Enter {
                let text = &text_input.lines()[0];
                match input_type {
                    CreateSplitName => {
                        let next_input_type = if false {
                            Some(CreateBreakOrderingConfirm)
                        } else if !text.ends_with(".o") {
                            Some(CreateNoExtensionConfirm)
                        } else if !text.contains("/") || text.starts_with("/") {
                            Some(CreateTopLevelConfrirm)
                        } else {
                            self.add_split_at_current_item(text);
                            None
                        };
                        self.text_input = next_input_type.map(|t| {
                            (
                                TextArea::default(),
                                t,
                                text.to_string(),
                                t.make_input_description(),
                            )
                        });
                    }
                    CreateBreakOrderingConfirm
                    | CreateNoExtensionConfirm
                    | CreateTopLevelConfrirm
                    | DeleteConfirm
                    | QuitAskSave => {
                        let lowercase_answer = text.to_lowercase();
                        if lowercase_answer == "n" || lowercase_answer == "no" {
                            return input_type == QuitAskSave;
                        }
                        if lowercase_answer != "y" && lowercase_answer != "yes" {
                            self.text_input =
                                Some((TextArea::default(), input_type, previous_input, desc));
                            return false;
                        }
                        match input_type {
                            CreateBreakOrderingConfirm
                            | CreateNoExtensionConfirm
                            | CreateTopLevelConfrirm => {
                                self.add_split_at_current_item(&previous_input)
                            }
                            CreateSplitName => unreachable!(),
                            DeleteConfirm => self.remove_selected_split(),
                            QuitAskSave => {
                                self.save_file_list();
                                return true;
                            }
                            _ => {}
                        }
                    }
                    RenameSplit => {
                        let EditorEntryData::Object(ref object_name, _) =
                            self.items[self.selected_item].data.clone()
                        else {
                            return false;
                        };
                        self.file_list
                            .iter_mut()
                            .find(|(n, _)| n == object_name)
                            .unwrap()
                            .0 = text.clone();
                        self.update();
                    }
                    JumpToObject => {
                        if let Some(idx) = self.items.iter().position(|item| {
                            if let EditorEntryData::Object(name, _) = &item.data {
                                return name.contains(text);
                            }
                            false
                        }) {
                            self.update_selected(idx);
                        }
                    }
                }
            } else if key.code != KeyCode::Esc {
                text_input.input(key);
                self.text_input = Some((text_input, input_type, previous_input, desc));
            }
            return false;
        }
        match key.code {
            KeyCode::Up => {
                if self.selected_item > 0 {
                    self.update_selected(self.selected_item - 1);
                    return false;
                }
            }
            KeyCode::Down => {
                if self.selected_item < self.items.len() - 1 {
                    self.update_selected(self.selected_item + 1);
                    return false;
                }
            }
            KeyCode::Enter => {
                if let EditorEntryData::Object(ref name, _) = self.items[self.selected_item].data {
                    if self.expanded_objects.contains(name.as_str()) {
                        self.expanded_objects.remove(name.as_str());
                    } else {
                        self.expanded_objects.insert(name.clone());
                    }
                    self.update();
                }
            }

            KeyCode::Char(ch) if matches!(ch, 'D' | 'R' | '/') => {
                if matches!(
                    self.items[self.selected_item].data,
                    EditorEntryData::Object(_, _)
                ) {
                    let input_type = match ch {
                        'D' => DeleteConfirm,
                        'R' => RenameSplit,
                        '/' => JumpToObject,
                        _ => unreachable!(),
                    };
                    self.text_input = Some((
                        TextArea::default(),
                        input_type,
                        String::new(),
                        input_type.make_input_description(),
                    ));
                }
                return false;
            }

            KeyCode::Char('C') => {
                if matches!(
                    self.items[self.selected_item].data,
                    EditorEntryData::Function(_)
                ) {
                    self.text_input = Some((
                        TextArea::default(),
                        TextInputType::CreateSplitName,
                        String::new(),
                        TextInputType::make_input_description(CreateSplitName),
                    ));
                }
                return false;
            }

            KeyCode::Char('S') => self.save_file_list(),

            KeyCode::Char('Q') => {
                self.text_input = Some((
                    TextArea::default(),
                    TextInputType::QuitAskSave,
                    String::new(),
                    TextInputType::make_input_description(QuitAskSave),
                ));
            }

            _ => {
                return false;
            }
        };
        false
    }
}

struct EditorEntry {
    is_selected: bool,
    data: EditorEntryData,
}

impl EditorEntry {
    fn new(data: EditorEntryData) -> Self {
        Self {
            is_selected: false,
            data,
        }
    }

    fn to_list_item(&self, text_width: i32) -> ListItem<'static> {
        let spans = match &self.data {
            EditorEntryData::Object(name, is_expanded) => {
                let text = if *is_expanded {
                    format!("▼ {name}")
                } else {
                    format!("▶ {name}")
                };
                let style = if self.is_selected {
                    Style::new().black().bold().on_blue()
                } else {
                    Style::new().blue()
                };
                let padding_len = (text_width - text.len() as i32).max(0) as usize;
                let object_span = Span::styled(text, style);
                let padding_span = Span::raw(" ".repeat(padding_len));
                vec![object_span, padding_span]
            }
            EditorEntryData::Function(function) => {
                let style = if self.is_selected {
                    Style::new().black().bold().on_light_green()
                } else {
                    Style::new().light_green()
                };
                vec![
                    Span::raw(" ".repeat(4)),
                    Span::styled(function.to_string(), style),
                ]
            }
        };
        ListItem::new(Line::from(spans))
    }
}

#[derive(Clone)]
enum EditorEntryData {
    Object(String, bool),
    Function(functions::Info),
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum TextInputType {
    CreateSplitName,
    CreateBreakOrderingConfirm,
    CreateTopLevelConfrirm,
    CreateNoExtensionConfirm,
    DeleteConfirm,
    QuitAskSave,
    RenameSplit,
    JumpToObject,
}

impl TextInputType {
    fn make_input_description(self) -> &'static str {
        match self {
            Self::CreateSplitName | Self::RenameSplit => "Enter split name",
            Self::CreateBreakOrderingConfirm => {
                "Are you sure you want to break the alphabetical ordering of splits (y/n)?"
            }
            Self::CreateTopLevelConfrirm => {
                "Are you sure you want to create a top level split (y/n)?"
            }
            Self::CreateNoExtensionConfirm => {
                "Are you sure you want to create a split without a '.o' extension (y/n)?"
            }
            Self::DeleteConfirm => "Are you sure you want to delete this split (y/n)?",
            Self::QuitAskSave => "Do you want to save (y/n)?",
            Self::JumpToObject => "Enter search",
        }
    }
}
