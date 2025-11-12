use crate::debug::{log, log_debug, LogLevel};
use crate::licenses::{LicenseCompatibility, LicenseInfo};
use color_eyre::Result;
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Layout, Margin, Rect},
    style::{self, Color, Modifier, Style, Stylize},
    text::Text,
    widgets::{
        Block, BorderType, Cell, HighlightSpacing, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, TableState,
    },
    DefaultTerminal, Frame,
};
use style::palette::tailwind;
use unicode_width::UnicodeWidthStr;

const INFO_TEXT: [&str; 2] = [
    "(Esc) quit | (↑↓) move | (←→) columns | (s) sort mode",
    "(In sort mode: ←→ select column, Enter toggle sort, Esc/q exit sort)",
];

const ITEM_HEIGHT: usize = 4;

const TABLE_COLOUR: tailwind::Palette = tailwind::RED;

struct TableColors {
    buffer_bg: Color,
    header_bg: Color,
    header_fg: Color,
    row_fg: Color,
    selected_row_style_fg: Color,
    selected_column_style_fg: Color,
    selected_cell_style_fg: Color,
    normal_row_color: Color,
    alt_row_color: Color,
    footer_border_color: Color,
    compatible_color: Color,
    incompatible_color: Color,
    unknown_color: Color,
    osi_approved_color: Color,
    osi_not_approved_color: Color,
    osi_unknown_color: Color,
}

impl TableColors {
    const fn new(color: &tailwind::Palette) -> Self {
        Self {
            buffer_bg: tailwind::SLATE.c950,
            header_bg: color.c900,
            header_fg: tailwind::SLATE.c200,
            row_fg: tailwind::SLATE.c200,
            selected_row_style_fg: color.c400,
            selected_column_style_fg: color.c400,
            selected_cell_style_fg: color.c600,
            normal_row_color: tailwind::SLATE.c950,
            alt_row_color: tailwind::SLATE.c900,
            footer_border_color: color.c400,
            compatible_color: tailwind::GREEN.c500,
            incompatible_color: tailwind::RED.c500,
            unknown_color: tailwind::YELLOW.c500,
            osi_approved_color: tailwind::BLUE.c500,
            osi_not_approved_color: tailwind::ORANGE.c500,
            osi_unknown_color: tailwind::GRAY.c500,
        }
    }
}

/// Column sorting direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// Represents which column is currently being sorted
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SortColumn {
    Name,
    Version,
    License,
    Restrictive,
    Compatibility,
    OsiStatus,
}

impl SortColumn {
    /// Get all available sort columns in order
    pub fn all() -> &'static [SortColumn] {
        &[
            SortColumn::Name,
            SortColumn::Version,
            SortColumn::License,
            SortColumn::Restrictive,
            SortColumn::Compatibility,
            SortColumn::OsiStatus,
        ]
    }

    /// Get display name for the column
    pub fn display_name(&self) -> &'static str {
        match self {
            SortColumn::Name => "Name",
            SortColumn::Version => "Version",
            SortColumn::License => "License",
            SortColumn::Restrictive => "Restrictive",
            SortColumn::Compatibility => "Compatibility",
            SortColumn::OsiStatus => "OSI Status",
        }
    }
}

/// Application mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Normal,
    Sorting,
}

pub struct App {
    state: TableState,
    items: Vec<LicenseInfo>,
    longest_item_lens: (u16, u16, u16, u16, u16, u16), // Name, Version, License, Restrictive, Compatibility, OSI Status
    scroll_state: ScrollbarState,
    colors: TableColors,
    project_license: Option<String>,
    sort_column: Option<SortColumn>,
    sort_direction: SortDirection,
    mode: AppMode,
    sort_column_selection: usize, // Index in SortColumn::all()
}

impl App {
    pub fn new(license_data: Vec<LicenseInfo>, project_license: Option<String>) -> Self {
        log(LogLevel::Info, "Initializing TUI application");
        log_debug("License data for TUI", &license_data);
        log(
            LogLevel::Info,
            &format!("Project license: {project_license:?}"),
        );

        let data_vec = license_data;
        Self {
            state: TableState::default().with_selected(0),
            longest_item_lens: constraint_len_calculator(&data_vec),
            scroll_state: ScrollbarState::new((data_vec.len().saturating_sub(1)) * ITEM_HEIGHT),
            colors: TableColors::new(&TABLE_COLOUR),
            items: data_vec,
            project_license,
            sort_column: None,
            sort_direction: SortDirection::Ascending,
            mode: AppMode::Normal,
            sort_column_selection: 0,
        }
    }

    pub fn next_row(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
        self.scroll_state = self.scroll_state.position(i * ITEM_HEIGHT);
        log(LogLevel::Info, &format!("Selected row: {i}"));
    }

    pub fn previous_row(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
        self.scroll_state = self.scroll_state.position(i * ITEM_HEIGHT);
        log(LogLevel::Info, &format!("Selected row: {i}"));
    }

    pub fn next_column(&mut self) {
        self.state.select_next_column();
        log(LogLevel::Info, "Selected next column");
    }

    pub fn previous_column(&mut self) {
        self.state.select_previous_column();
        log(LogLevel::Info, "Selected previous column");
    }

    /// Enter sort mode
    pub fn enter_sort_mode(&mut self) {
        self.mode = AppMode::Sorting;
        // Start selection at current sort column if one exists, otherwise first column
        self.sort_column_selection = if let Some(col) = self.sort_column {
            SortColumn::all()
                .iter()
                .position(|&c| c == col)
                .unwrap_or(0)
        } else {
            0
        };
        log(LogLevel::Info, "Entered sort mode");
    }

    /// Exit sort mode without applying changes
    pub fn exit_sort_mode(&mut self) {
        self.mode = AppMode::Normal;
        log(LogLevel::Info, "Exited sort mode");
    }

    /// Move to next column in sort selection
    pub fn next_sort_column(&mut self) {
        if self.sort_column_selection < SortColumn::all().len().saturating_sub(1) {
            self.sort_column_selection += 1;
            log(
                LogLevel::Info,
                &format!("Sort column selection: {}", self.sort_column_selection),
            );
        }
    }

    /// Move to previous column in sort selection
    pub fn previous_sort_column(&mut self) {
        if self.sort_column_selection > 0 {
            self.sort_column_selection -= 1;
            log(
                LogLevel::Info,
                &format!("Sort column selection: {}", self.sort_column_selection),
            );
        }
    }

    /// Apply sort on currently selected column
    pub fn apply_current_sort(&mut self) {
        let column = SortColumn::all()[self.sort_column_selection];

        // If clicking the same column, toggle direction; otherwise set new column with ascending
        if self.sort_column == Some(column) {
            self.sort_direction = match self.sort_direction {
                SortDirection::Ascending => SortDirection::Descending,
                SortDirection::Descending => SortDirection::Ascending,
            };
        } else {
            self.sort_column = Some(column);
            self.sort_direction = SortDirection::Ascending;
        }

        self.apply_sort();
        self.exit_sort_mode();
        log(
            LogLevel::Info,
            &format!(
                "Sorted by {:?} in {:?} direction",
                self.sort_column, self.sort_direction
            ),
        );
    }

    /// Compare two version strings, handling 'v' prefix and semantic versioning
    fn compare_versions(a: &str, b: &str, ascending: bool) -> std::cmp::Ordering {
        // Remove 'v' prefix if present
        let a_version = a.trim_start_matches('v');
        let b_version = b.trim_start_matches('v');

        match (
            semver::Version::parse(a_version),
            semver::Version::parse(b_version),
        ) {
            // Both are valid semantic versions - compare semantically
            (Ok(v_a), Ok(v_b)) => v_a.cmp(&v_b),
            // One is valid semver, one isn't
            (Ok(_), Err(_)) => {
                // In ascending: semver comes first (Less)
                // In descending: semver comes last (Greater)
                if ascending {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            }
            (Err(_), Ok(_)) => {
                if ascending {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Less
                }
            }
            // Neither are valid semver - compare as strings
            (Err(_), Err(_)) => a_version.cmp(b_version),
        }
    }

    /// Apply the current sort to the items
    fn apply_sort(&mut self) {
        if let Some(column) = self.sort_column {
            let ascending = self.sort_direction == SortDirection::Ascending;

            match column {
                SortColumn::Name => {
                    self.items.sort_by(|a, b| {
                        let ord = a.name.cmp(&b.name);
                        if ascending {
                            ord
                        } else {
                            ord.reverse()
                        }
                    });
                }
                SortColumn::Version => {
                    self.items
                        .sort_by(|a, b| Self::compare_versions(&a.version, &b.version, ascending));
                }
                SortColumn::License => {
                    self.items.sort_by(|a, b| {
                        let ord = a.get_license().cmp(&b.get_license());
                        if ascending {
                            ord
                        } else {
                            ord.reverse()
                        }
                    });
                }
                SortColumn::Restrictive => {
                    self.items.sort_by(|a, b| {
                        let ord = a.is_restrictive.cmp(&b.is_restrictive);
                        if ascending {
                            ord
                        } else {
                            ord.reverse()
                        }
                    });
                }
                SortColumn::Compatibility => {
                    self.items.sort_by(|a, b| {
                        let ord =
                            format!("{:?}", a.compatibility).cmp(&format!("{:?}", b.compatibility));
                        if ascending {
                            ord
                        } else {
                            ord.reverse()
                        }
                    });
                }
                SortColumn::OsiStatus => {
                    self.items.sort_by(|a, b| {
                        let ord = format!("{:?}", a.osi_status).cmp(&format!("{:?}", b.osi_status));
                        if ascending {
                            ord
                        } else {
                            ord.reverse()
                        }
                    });
                }
            }

            // Reset selection to top when sorting
            self.state.select(Some(0));
            self.scroll_state =
                ScrollbarState::new((self.items.len().saturating_sub(1)) * ITEM_HEIGHT);
        }
    }

    pub fn set_colors(&mut self) {
        self.colors = TableColors::new(&TABLE_COLOUR);
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        log(LogLevel::Info, "Starting TUI application loop");

        loop {
            // Render the current state
            terminal.draw(|frame| self.draw(frame))?;

            // Handle input events
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match self.mode {
                        AppMode::Normal => match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                log(LogLevel::Info, "Quitting TUI application");
                                return Ok(());
                            }
                            KeyCode::Char('j') | KeyCode::Down => self.next_row(),
                            KeyCode::Char('k') | KeyCode::Up => self.previous_row(),
                            KeyCode::Char('l') | KeyCode::Right => self.next_column(),
                            KeyCode::Char('h') | KeyCode::Left => self.previous_column(),
                            KeyCode::Char('s') => self.enter_sort_mode(),
                            _ => {}
                        },
                        AppMode::Sorting => match key.code {
                            KeyCode::Left => self.previous_sort_column(),
                            KeyCode::Right => self.next_sort_column(),
                            KeyCode::Char('h') => self.previous_sort_column(),
                            KeyCode::Char('l') => self.next_sort_column(),
                            KeyCode::Enter => self.apply_current_sort(),
                            KeyCode::Char('q') | KeyCode::Esc => self.exit_sort_mode(),
                            _ => {}
                        },
                    }
                }
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let vertical = &Layout::vertical([Constraint::Min(5), Constraint::Length(4)]);
        let rects = vertical.split(frame.area());

        self.set_colors();

        self.render_table(frame, rects[0]);
        self.render_scrollbar(frame, rects[0]);
        self.render_footer(frame, rects[1]);
    }

    fn render_table(&mut self, frame: &mut Frame, area: Rect) {
        log(LogLevel::Info, "Rendering table");

        let header_style = Style::default()
            .fg(self.colors.header_fg)
            .bg(self.colors.header_bg);
        let selected_row_style = Style::default()
            .add_modifier(Modifier::REVERSED)
            .fg(self.colors.selected_row_style_fg);
        let selected_col_style = Style::default().fg(self.colors.selected_column_style_fg);
        let selected_cell_style = Style::default()
            .add_modifier(Modifier::REVERSED)
            .fg(self.colors.selected_cell_style_fg);

        // Add Compatibility and OSI Status columns to header
        // Add sort indicators to column headers if sorting is active
        let header = SortColumn::all()
            .iter()
            .map(|col| {
                let mut display_name = col.display_name().to_string();

                // Add sort direction indicator if this column is sorted
                if let Some(sort_col) = self.sort_column {
                    if sort_col == *col {
                        let direction = match self.sort_direction {
                            SortDirection::Ascending => " ↑",
                            SortDirection::Descending => " ↓",
                        };
                        display_name.push_str(direction);
                    }
                }

                Cell::from(display_name)
            })
            .collect::<Row>()
            .style(header_style)
            .height(1);

        let rows = self.items.iter().enumerate().map(|(i, data)| {
            let color = match i % 2 {
                0 => self.colors.normal_row_color,
                _ => self.colors.alt_row_color,
            };

            // Style compatibility text based on its value
            let compatibility_text = match data.compatibility {
                LicenseCompatibility::Compatible => {
                    Text::from(format!("\n{}\n", "Compatible")).fg(self.colors.compatible_color)
                }
                LicenseCompatibility::Incompatible => {
                    Text::from(format!("\n{}\n", "Incompatible")).fg(self.colors.incompatible_color)
                }
                LicenseCompatibility::Unknown => {
                    Text::from(format!("\n{}\n", "Unknown")).fg(self.colors.unknown_color)
                }
            };

            // Style OSI status text based on its value
            let osi_status_text = match data.osi_status {
                crate::licenses::OsiStatus::Approved => {
                    Text::from(format!("\n{}\n", "approved")).fg(self.colors.osi_approved_color)
                }
                crate::licenses::OsiStatus::NotApproved => {
                    Text::from(format!("\n{}\n", "not-approved"))
                        .fg(self.colors.osi_not_approved_color)
                }
                crate::licenses::OsiStatus::Unknown => {
                    Text::from(format!("\n{}\n", "unknown")).fg(self.colors.osi_unknown_color)
                }
            };

            let row = Row::new([
                Cell::from(Text::from(format!("\n{}\n", data.name))),
                Cell::from(Text::from(format!("\n{}\n", data.version))),
                Cell::from(Text::from(format!("\n{}\n", data.get_license()))),
                Cell::from(Text::from(format!("\n{}\n", data.is_restrictive()))),
                Cell::from(compatibility_text),
                Cell::from(osi_status_text),
            ])
            .style(Style::new().fg(self.colors.row_fg).bg(color))
            .height(4);

            row
        });

        let bar = " █ ";
        let t = Table::new(
            rows,
            [
                // + 1 is for padding.
                Constraint::Length(self.longest_item_lens.0 + 1),
                Constraint::Min(self.longest_item_lens.1 + 1),
                Constraint::Min(self.longest_item_lens.2),
                Constraint::Min(self.longest_item_lens.3),
                Constraint::Min(self.longest_item_lens.4), // Compatibility column
                Constraint::Min(self.longest_item_lens.5), // OSI Status column
            ],
        )
        .header(header)
        .row_highlight_style(selected_row_style)
        .column_highlight_style(selected_col_style)
        .cell_highlight_style(selected_cell_style)
        .highlight_symbol(Text::from(vec![
            "".into(),
            bar.into(),
            bar.into(),
            "".into(),
        ]))
        .bg(self.colors.buffer_bg)
        .highlight_spacing(HighlightSpacing::Always);

        frame.render_stateful_widget(t, area, &mut self.state);

        log(
            LogLevel::Info,
            &format!("Table rendered with {} rows", self.items.len()),
        );
    }

    fn render_scrollbar(&mut self, frame: &mut Frame, area: Rect) {
        frame.render_stateful_widget(
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            area.inner(Margin {
                vertical: 1,
                horizontal: 1,
            }),
            &mut self.scroll_state,
        );
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        if self.mode == AppMode::Sorting {
            // Show sort mode UI
            let mut column_display = String::new();
            for (idx, col) in SortColumn::all().iter().enumerate() {
                if idx == self.sort_column_selection {
                    column_display.push_str(&format!("[>{}< ] ", col.display_name()));
                } else {
                    column_display.push_str(&format!(" {}  ", col.display_name()));
                }
            }

            let current_sort = if let Some(col) = self.sort_column {
                let dir = match self.sort_direction {
                    SortDirection::Ascending => "↑",
                    SortDirection::Descending => "↓",
                };
                format!("Current: {} {}", col.display_name(), dir)
            } else {
                "Current: None".to_string()
            };

            let footer_text = format!("Sort Mode\n{column_display}\n{current_sort}");

            let info_footer = Paragraph::new(Text::from(footer_text))
                .style(
                    Style::new()
                        .fg(self.colors.header_fg)
                        .bg(self.colors.header_bg),
                )
                .centered()
                .block(
                    Block::bordered()
                        .border_type(BorderType::Double)
                        .border_style(Style::new().fg(self.colors.selected_row_style_fg)),
                );
            frame.render_widget(info_footer, area);
        } else {
            // Normal mode footer
            // Add sort indicator if a column is being sorted
            let sort_indicator = if let Some(column) = self.sort_column {
                let direction = match self.sort_direction {
                    SortDirection::Ascending => "↑",
                    SortDirection::Descending => "↓",
                };
                format!(" | Sort: {} {}", column.display_name(), direction)
            } else {
                String::new()
            };

            // Add project license information to footer if available
            let license_text = if let Some(ref license) = self.project_license {
                format!("Project: {license}")
            } else {
                "Project: Unknown".to_string()
            };

            let footer_text = format!("{license_text} | {}{sort_indicator}", INFO_TEXT[0]);
            let help_text = format!("\n{}", INFO_TEXT[1]);

            let info_footer = Paragraph::new(Text::from(format!("{footer_text}{help_text}")))
                .style(
                    Style::new()
                        .fg(self.colors.row_fg)
                        .bg(self.colors.buffer_bg),
                )
                .centered()
                .block(
                    Block::bordered()
                        .border_type(BorderType::Double)
                        .border_style(Style::new().fg(self.colors.footer_border_color)),
                );
            frame.render_widget(info_footer, area);
        }
    }
}

fn constraint_len_calculator(items: &[LicenseInfo]) -> (u16, u16, u16, u16, u16, u16) {
    log(LogLevel::Info, "Calculating column widths for table");

    let name_len = items
        .iter()
        .map(LicenseInfo::name)
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0);

    let version_len = items
        .iter()
        .map(LicenseInfo::version)
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0);

    let license_len = items
        .iter()
        .map(|info| info.get_license())
        .map(|s| s.width())
        .max()
        .unwrap_or(0);

    let restricted_len = "true".width().max("false".width());

    // Calculate width for the Compatibility column
    let compatibility_len = ["Compatible", "Incompatible", "Unknown"]
        .iter()
        .map(|s| s.width())
        .max()
        .unwrap_or(0);

    // Calculate width for the OSI Status column
    let osi_status_len = ["approved", "not-approved", "unknown"]
        .iter()
        .map(|s| s.width())
        .max()
        .unwrap_or(0);

    #[allow(clippy::cast_possible_truncation)]
    let result = (
        name_len as u16,
        version_len as u16,
        license_len as u16,
        restricted_len as u16,
        compatibility_len as u16,
        osi_status_len as u16,
    );

    log(LogLevel::Info, &format!("Table column widths: {result:?}"));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_new() {
        let test_data = vec![LicenseInfo {
            name: "test_package".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
            osi_status: crate::licenses::OsiStatus::Approved,
        }];

        let app = App::new(test_data.clone(), Some("MIT".to_string()));

        assert_eq!(app.items.len(), 1);
        assert_eq!(app.project_license, Some("MIT".to_string()));
        assert_eq!(app.state.selected(), Some(0));

        let app_no_license = App::new(test_data, None);
        assert!(app_no_license.project_license.is_none());
    }

    #[test]
    fn test_app_new_empty_data() {
        let test_data = vec![];
        let app = App::new(test_data, Some("Apache-2.0".to_string()));

        assert_eq!(app.items.len(), 0);
        assert_eq!(app.project_license, Some("Apache-2.0".to_string()));
        assert_eq!(app.state.selected(), Some(0));
    }

    #[test]
    fn test_app_navigation() {
        let test_data = vec![
            LicenseInfo {
                name: "package1".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "package2".to_string(),
                version: "2.0.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "package3".to_string(),
                version: "3.0.0".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let mut app = App::new(test_data, None);

        assert_eq!(app.state.selected(), Some(0));

        app.next_row();
        assert_eq!(app.state.selected(), Some(1));

        app.next_row();
        assert_eq!(app.state.selected(), Some(2));

        app.next_row();
        assert_eq!(app.state.selected(), Some(0));

        app.previous_row();
        assert_eq!(app.state.selected(), Some(2));

        app.previous_row();
        assert_eq!(app.state.selected(), Some(1));

        app.previous_row();
        assert_eq!(app.state.selected(), Some(0));

        app.previous_row();
        assert_eq!(app.state.selected(), Some(2));

        app.next_column();
        app.previous_column();
    }

    #[test]
    fn test_app_navigation_single_item() {
        let test_data = vec![LicenseInfo {
            name: "single_package".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
            osi_status: crate::licenses::OsiStatus::Approved,
        }];

        let mut app = App::new(test_data, None);

        assert_eq!(app.state.selected(), Some(0));

        app.next_row();
        assert_eq!(app.state.selected(), Some(0));

        app.previous_row();
        assert_eq!(app.state.selected(), Some(0));
    }

    #[test]
    fn test_app_navigation_empty_list() {
        let test_data = vec![];
        let mut app = App::new(test_data, None);

        assert_eq!(app.state.selected(), Some(0));

        app.next_row();
        assert_eq!(app.state.selected(), Some(0));

        app.previous_row();
        assert_eq!(app.state.selected(), Some(0));
    }

    #[test]
    fn test_constraint_len_calculator() {
        let test_data = vec![
            LicenseInfo {
                name: "very_long_package_name_that_exceeds_normal_length".to_string(),
                version: "1.0.0-beta.1+build.123".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "short".to_string(),
                version: "2.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let (name_len, version_len, license_len, restricted_len, compatibility_len, _osi_len) =
            constraint_len_calculator(&test_data);

        assert_eq!(
            name_len,
            "very_long_package_name_that_exceeds_normal_length".len() as u16
        );
        assert_eq!(version_len, "1.0.0-beta.1+build.123".len() as u16);
        assert_eq!(license_len, "Apache-2.0".len() as u16);
        assert_eq!(restricted_len, "false".len() as u16);
        assert_eq!(compatibility_len, "Incompatible".len() as u16);
    }

    #[test]
    fn test_constraint_len_calculator_empty() {
        let test_data = vec![];
        let (name_len, version_len, license_len, restricted_len, compatibility_len, _osi_len) =
            constraint_len_calculator(&test_data);

        assert_eq!(name_len, 0);
        assert_eq!(version_len, 0);
        assert_eq!(license_len, 0);
        assert_eq!(restricted_len, "false".len() as u16);
        assert_eq!(compatibility_len, "Incompatible".len() as u16);
    }

    #[test]
    fn test_constraint_len_calculator_unicode() {
        let test_data = vec![LicenseInfo {
            name: "package_with_émojis_🚀_and_ünïcödé".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
            osi_status: crate::licenses::OsiStatus::Approved,
        }];

        let (name_len, _, _, _, _, _) = constraint_len_calculator(&test_data);

        assert!(name_len > 0);
    }

    #[test]
    fn test_constraint_len_calculator_all_compatibility_types() {
        let test_data = vec![
            LicenseInfo {
                name: "compatible".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "incompatible".to_string(),
                version: "1.0.0".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "unknown".to_string(),
                version: "1.0.0".to_string(),
                license: Some("Custom".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Unknown,
                osi_status: crate::licenses::OsiStatus::Unknown,
            },
        ];

        let (_, _, _, _, compatibility_len, _) = constraint_len_calculator(&test_data);

        assert_eq!(compatibility_len, "Incompatible".len() as u16);
    }

    #[test]
    fn test_constraint_len_calculator_restrictive_values() {
        let test_data = vec![
            LicenseInfo {
                name: "package".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: true, // true
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "package2".to_string(),
                version: "1.0.0".to_string(),
                license: Some("Apache".to_string()),
                is_restrictive: false, // false
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let (_, _, _, restricted_len, _, _) = constraint_len_calculator(&test_data);

        assert_eq!(restricted_len, "false".len() as u16);
    }

    #[test]
    fn test_item_height_constant() {
        assert_eq!(ITEM_HEIGHT, 4);
    }

    #[test]
    fn test_info_text_constant() {
        assert_eq!(INFO_TEXT.len(), 2);
        assert!(INFO_TEXT[0].contains("Esc"));
        assert!(INFO_TEXT[0].contains("quit"));
        assert!(INFO_TEXT[0].contains("sort mode"));
        assert!(INFO_TEXT[1].contains("sort mode"));
        assert!(INFO_TEXT[1].contains("Enter"));
    }

    #[test]
    fn test_app_longest_item_lens_calculation() {
        let test_data = vec![
            LicenseInfo {
                name: "short".to_string(),
                version: "1.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "much_longer_name".to_string(),
                version: "1.0.0-beta".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let app = App::new(test_data, None);

        assert_eq!(app.longest_item_lens.0, "much_longer_name".len() as u16);
        assert_eq!(app.longest_item_lens.1, "1.0.0-beta".len() as u16);
        assert_eq!(app.longest_item_lens.2, "Apache-2.0".len() as u16);
        assert_eq!(app.longest_item_lens.3, "false".len() as u16);
        assert_eq!(app.longest_item_lens.4, "Incompatible".len() as u16);
    }

    #[test]
    fn test_sort_by_name() {
        let test_data = vec![
            LicenseInfo {
                name: "zebra".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "apple".to_string(),
                version: "2.0.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "banana".to_string(),
                version: "3.0.0".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let mut app = App::new(test_data, None);
        app.enter_sort_mode();
        // SortColumn::Name is at index 0, so no navigation needed
        app.apply_current_sort();

        assert_eq!(app.items[0].name, "apple");
        assert_eq!(app.items[1].name, "banana");
        assert_eq!(app.items[2].name, "zebra");
        assert_eq!(app.sort_column, Some(SortColumn::Name));
        assert_eq!(app.sort_direction, SortDirection::Ascending);
        assert_eq!(app.mode, AppMode::Normal);
    }

    #[test]
    fn test_sort_by_name_descending() {
        let test_data = vec![
            LicenseInfo {
                name: "apple".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "zebra".to_string(),
                version: "2.0.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let mut app = App::new(test_data, None);
        app.enter_sort_mode();
        app.apply_current_sort(); // First sort ascending
        app.enter_sort_mode();
        app.apply_current_sort(); // Toggle to descending

        assert_eq!(app.items[0].name, "zebra");
        assert_eq!(app.items[1].name, "apple");
        assert_eq!(app.sort_direction, SortDirection::Descending);
    }

    #[test]
    fn test_sort_by_restrictive() {
        let test_data = vec![
            LicenseInfo {
                name: "package1".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "package2".to_string(),
                version: "2.0.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let mut app = App::new(test_data, None);
        app.enter_sort_mode();
        // Navigate to Restrictive column (index 3)
        app.next_sort_column(); // 1
        app.next_sort_column(); // 2
        app.next_sort_column(); // 3
        app.apply_current_sort();

        // False comes before True in ascending order
        assert!(!app.items[0].is_restrictive);
        assert!(app.items[1].is_restrictive);
        assert_eq!(app.sort_column, Some(SortColumn::Restrictive));
    }

    #[test]
    fn test_sort_mode_navigation() {
        let test_data = vec![LicenseInfo {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
            osi_status: crate::licenses::OsiStatus::Approved,
        }];

        let mut app = App::new(test_data, None);
        assert_eq!(app.mode, AppMode::Normal);

        app.enter_sort_mode();
        assert_eq!(app.mode, AppMode::Sorting);
        assert_eq!(app.sort_column_selection, 0);

        app.next_sort_column();
        assert_eq!(app.sort_column_selection, 1);

        app.previous_sort_column();
        assert_eq!(app.sort_column_selection, 0);

        app.exit_sort_mode();
        assert_eq!(app.mode, AppMode::Normal);
    }

    #[test]
    fn test_sort_direction_toggle() {
        let test_data = vec![LicenseInfo {
            name: "package".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
            osi_status: crate::licenses::OsiStatus::Approved,
        }];

        let mut app = App::new(test_data, None);

        // First sort should be Ascending
        app.enter_sort_mode();
        app.apply_current_sort();
        assert_eq!(app.sort_direction, SortDirection::Ascending);

        // Second sort on same column should toggle to Descending
        app.enter_sort_mode();
        app.apply_current_sort();
        assert_eq!(app.sort_direction, SortDirection::Descending);

        // Third sort should toggle back to Ascending
        app.enter_sort_mode();
        app.apply_current_sort();
        assert_eq!(app.sort_direction, SortDirection::Ascending);
    }

    #[test]
    fn test_sort_column_change() {
        let test_data = vec![
            LicenseInfo {
                name: "zebra".to_string(),
                version: "1.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "apple".to_string(),
                version: "5.0.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let mut app = App::new(test_data, None);

        // Sort by Name
        app.enter_sort_mode();
        app.apply_current_sort();
        assert_eq!(app.items[0].name, "apple");
        assert_eq!(app.sort_direction, SortDirection::Ascending);

        // Change to sort by Version - should reset to Ascending
        app.enter_sort_mode();
        app.next_sort_column(); // Navigate to Version (index 1)
        app.apply_current_sort();
        assert_eq!(app.sort_column, Some(SortColumn::Version));
        assert_eq!(app.sort_direction, SortDirection::Ascending);
    }

    #[test]
    fn test_initial_sort_state() {
        let test_data = vec![LicenseInfo {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            license: Some("MIT".to_string()),
            is_restrictive: false,
            compatibility: LicenseCompatibility::Compatible,
            osi_status: crate::licenses::OsiStatus::Approved,
        }];

        let app = App::new(test_data, None);

        assert_eq!(app.sort_column, None);
        assert_eq!(app.sort_direction, SortDirection::Ascending);
        assert_eq!(app.mode, AppMode::Normal);
        assert_eq!(app.sort_column_selection, 0);
    }

    #[test]
    fn test_sort_by_version_with_v_prefix() {
        let test_data = vec![
            LicenseInfo {
                name: "package1".to_string(),
                version: "v3.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "package2".to_string(),
                version: "v1.0.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "package3".to_string(),
                version: "v2.5.0".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let mut app = App::new(test_data, None);
        app.enter_sort_mode();
        // Navigate to Version column (index 1)
        app.next_sort_column();
        app.apply_current_sort();

        // Should be sorted as v1.0.0, v2.5.0, v3.0.0
        assert_eq!(app.items[0].version, "v1.0.0");
        assert_eq!(app.items[1].version, "v2.5.0");
        assert_eq!(app.items[2].version, "v3.0.0");
        assert_eq!(app.sort_column, Some(SortColumn::Version));
        assert_eq!(app.sort_direction, SortDirection::Ascending);
    }

    #[test]
    fn test_sort_by_version_mixed_prefix() {
        let test_data = vec![
            LicenseInfo {
                name: "package1".to_string(),
                version: "3.0.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "package2".to_string(),
                version: "v1.5.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "package3".to_string(),
                version: "v2.0.0".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let mut app = App::new(test_data, None);
        app.enter_sort_mode();
        // Navigate to Version column (index 1)
        app.next_sort_column();
        app.apply_current_sort();

        // Should be sorted as v1.5.0, v2.0.0, 3.0.0 (semantic versions first, then non-semantic)
        assert_eq!(app.items[0].version, "v1.5.0");
        assert_eq!(app.items[1].version, "v2.0.0");
        assert_eq!(app.items[2].version, "3.0.0");
    }

    #[test]
    fn test_sort_by_version_descending() {
        let test_data = vec![
            LicenseInfo {
                name: "package1".to_string(),
                version: "v10.14.0".to_string(),
                license: Some("MIT".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "package2".to_string(),
                version: "0.14".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
            LicenseInfo {
                name: "package3".to_string(),
                version: "2015.7".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
            },
        ];

        let mut app = App::new(test_data, None);
        app.enter_sort_mode();
        // Navigate to Version column (index 1)
        app.next_sort_column();
        app.apply_current_sort(); // First sort on Version (ascending)

        // Enter sort mode again - it should remember we're on Version
        app.enter_sort_mode();
        // We should already be on Version (index 1), so no need to navigate
        app.apply_current_sort(); // Toggle to Descending

        // In descending: non-semantic versions come BEFORE semantic versions
        // String order reversed: "2015.7" < "0.14" when reversed
        assert_eq!(app.items[0].version, "0.14");
        assert_eq!(app.items[1].version, "2015.7");
        assert_eq!(app.items[2].version, "v10.14.0");
        assert_eq!(app.sort_direction, SortDirection::Descending);
    }
}
