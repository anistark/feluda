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
    "(Esc) quit | (↑) move up | (↓) move down | (←) move left | (→) move right",
    "(r) restrictive | (i) incompatible | (c) compatible | (a) osi-approved | (n) osi-not-approved | (u) osi-unknown | (x) clear filters",
];

const ITEM_HEIGHT: usize = 4;

const TABLE_COLOUR: tailwind::Palette = tailwind::RED;

#[derive(Debug, Clone, Default)]
struct FilterState {
    show_restrictive_only: bool,
    show_incompatible_only: bool,
    show_compatible_only: bool,
    show_osi_approved_only: bool,
    show_osi_not_approved_only: bool,
    show_osi_unknown_only: bool,
}

impl FilterState {
    fn is_any_active(&self) -> bool {
        self.show_restrictive_only
            || self.show_incompatible_only
            || self.show_compatible_only
            || self.show_osi_approved_only
            || self.show_osi_not_approved_only
            || self.show_osi_unknown_only
    }

    fn clear_all(&mut self) {
        self.show_restrictive_only = false;
        self.show_incompatible_only = false;
        self.show_compatible_only = false;
        self.show_osi_approved_only = false;
        self.show_osi_not_approved_only = false;
        self.show_osi_unknown_only = false;
    }

    fn matches(&self, item: &LicenseInfo) -> bool {
        if !self.is_any_active() {
            return true;
        }

        let mut matches = true;

        // If any restrictive filter is active, check it
        if self.show_restrictive_only && !item.is_restrictive {
            matches = false;
        }

        if self.show_incompatible_only || self.show_compatible_only {
            let compat_match = match item.compatibility {
                LicenseCompatibility::Incompatible => self.show_incompatible_only,
                LicenseCompatibility::Compatible => self.show_compatible_only,
                LicenseCompatibility::Unknown => false,
            };
            if !compat_match {
                matches = false;
            }
        }

        if self.show_osi_approved_only
            || self.show_osi_not_approved_only
            || self.show_osi_unknown_only
        {
            let osi_match = match item.osi_status {
                crate::licenses::OsiStatus::Approved => self.show_osi_approved_only,
                crate::licenses::OsiStatus::NotApproved => self.show_osi_not_approved_only,
                crate::licenses::OsiStatus::Unknown => self.show_osi_unknown_only,
            };
            if !osi_match {
                matches = false;
            }
        }

        matches
    }
}

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

pub struct App {
    state: TableState,
    items: Vec<LicenseInfo>,
    longest_item_lens: (u16, u16, u16, u16, u16, u16), // Name, Version, License, Restrictive, Compatibility, OSI Status
    scroll_state: ScrollbarState,
    colors: TableColors,
    project_license: Option<String>,
    filters: FilterState,
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
            filters: FilterState::default(),
        }
    }

    fn get_filtered_items(&self) -> Vec<&LicenseInfo> {
        self.items
            .iter()
            .filter(|item| self.filters.matches(item))
            .collect()
    }

    fn update_scroll_state(&mut self) {
        let filtered_count = self.get_filtered_items().len();
        self.scroll_state = ScrollbarState::new((filtered_count.saturating_sub(1)) * ITEM_HEIGHT);
    }

    pub fn next_row(&mut self) {
        let filtered_count = self.get_filtered_items().len();
        let i = match self.state.selected() {
            Some(i) => {
                if i >= filtered_count.saturating_sub(1) {
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
        let filtered_count = self.get_filtered_items().len();
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    filtered_count.saturating_sub(1)
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

    pub fn toggle_restrictive_filter(&mut self) {
        self.filters.show_restrictive_only = !self.filters.show_restrictive_only;
        log(
            LogLevel::Info,
            &format!("Restrictive filter: {}", self.filters.show_restrictive_only),
        );
        self.update_scroll_state();
        self.state.select(Some(0));
    }

    pub fn toggle_incompatible_filter(&mut self) {
        self.filters.show_incompatible_only = !self.filters.show_incompatible_only;
        log(
            LogLevel::Info,
            &format!(
                "Incompatible filter: {}",
                self.filters.show_incompatible_only
            ),
        );
        self.update_scroll_state();
        self.state.select(Some(0));
    }

    pub fn toggle_compatible_filter(&mut self) {
        self.filters.show_compatible_only = !self.filters.show_compatible_only;
        log(
            LogLevel::Info,
            &format!("Compatible filter: {}", self.filters.show_compatible_only),
        );
        self.update_scroll_state();
        self.state.select(Some(0));
    }

    pub fn toggle_osi_approved_filter(&mut self) {
        self.filters.show_osi_approved_only = !self.filters.show_osi_approved_only;
        log(
            LogLevel::Info,
            &format!(
                "OSI Approved filter: {}",
                self.filters.show_osi_approved_only
            ),
        );
        self.update_scroll_state();
        self.state.select(Some(0));
    }

    pub fn toggle_osi_not_approved_filter(&mut self) {
        self.filters.show_osi_not_approved_only = !self.filters.show_osi_not_approved_only;
        log(
            LogLevel::Info,
            &format!(
                "OSI Not Approved filter: {}",
                self.filters.show_osi_not_approved_only
            ),
        );
        self.update_scroll_state();
        self.state.select(Some(0));
    }

    pub fn toggle_osi_unknown_filter(&mut self) {
        self.filters.show_osi_unknown_only = !self.filters.show_osi_unknown_only;
        log(
            LogLevel::Info,
            &format!("OSI Unknown filter: {}", self.filters.show_osi_unknown_only),
        );
        self.update_scroll_state();
        self.state.select(Some(0));
    }

    pub fn clear_filters(&mut self) {
        self.filters.clear_all();
        log(LogLevel::Info, "All filters cleared");
        self.update_scroll_state();
        self.state.select(Some(0));
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
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            log(LogLevel::Info, "Quitting TUI application");
                            return Ok(());
                        }
                        KeyCode::Char('j') | KeyCode::Down => self.next_row(),
                        KeyCode::Char('k') | KeyCode::Up => self.previous_row(),
                        KeyCode::Char('l') | KeyCode::Right => self.next_column(),
                        KeyCode::Char('h') | KeyCode::Left => self.previous_column(),
                        KeyCode::Char('r') => self.toggle_restrictive_filter(),
                        KeyCode::Char('i') => self.toggle_incompatible_filter(),
                        KeyCode::Char('c') => self.toggle_compatible_filter(),
                        KeyCode::Char('a') => self.toggle_osi_approved_filter(),
                        KeyCode::Char('n') => self.toggle_osi_not_approved_filter(),
                        KeyCode::Char('u') => self.toggle_osi_unknown_filter(),
                        KeyCode::Char('x') => self.clear_filters(),
                        _ => {}
                    }
                }
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        // Add space for filter bar if filters are active
        let vertical = if self.filters.is_any_active() {
            Layout::vertical([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(5),
            ])
        } else {
            Layout::vertical([
                Constraint::Length(0),
                Constraint::Min(5),
                Constraint::Length(5),
            ])
        };
        let rects = vertical.split(frame.area());

        self.set_colors();

        if self.filters.is_any_active() {
            self.render_filter_bar(frame, rects[0]);
        }
        self.render_table(frame, rects[1]);
        self.render_scrollbar(frame, rects[1]);
        self.render_footer(frame, rects[2]);
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
        let header = [
            "Name",
            "Version",
            "License",
            "Restrictive",
            "Compatibility",
            "OSI Status",
        ]
        .into_iter()
        .map(Cell::from)
        .collect::<Row>()
        .style(header_style)
        .height(1);

        // Use filtered items instead of all items
        let filtered_items = self.get_filtered_items();
        let filtered_count = filtered_items.len();
        let total_count = self.items.len();

        let rows = filtered_items.iter().enumerate().map(|(i, data)| {
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
            &format!(
                "Table rendered with {} rows (filtered from {} total)",
                filtered_count, total_count
            ),
        );
    }

    fn render_filter_bar(&self, frame: &mut Frame, area: Rect) {
        let mut filter_tags = Vec::new();

        if self.filters.show_restrictive_only {
            filter_tags.push("Restrictive");
        }
        if self.filters.show_incompatible_only {
            filter_tags.push("Incompatible");
        }
        if self.filters.show_compatible_only {
            filter_tags.push("Compatible");
        }
        if self.filters.show_osi_approved_only {
            filter_tags.push("OSI-Approved");
        }
        if self.filters.show_osi_not_approved_only {
            filter_tags.push("OSI-NotApproved");
        }
        if self.filters.show_osi_unknown_only {
            filter_tags.push("OSI-Unknown");
        }

        let filter_text = format!("Active Filters: {}", filter_tags.join(", "));
        let filtered_count = self.get_filtered_items().len();
        let filter_info = format!(
            "{} | Showing {} of {} licenses",
            filter_text,
            filtered_count,
            self.items.len()
        );

        let filter_paragraph = Paragraph::new(Text::from(filter_info))
            .style(
                Style::new()
                    .fg(self.colors.footer_border_color)
                    .bg(self.colors.buffer_bg)
                    .add_modifier(Modifier::BOLD),
            )
            .centered()
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(self.colors.footer_border_color)),
            );
        frame.render_widget(filter_paragraph, area);
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
        // Add project license information to footer if available
        let license_text = if let Some(ref license) = self.project_license {
            format!("Project License: {}", license)
        } else {
            "Project License: Unknown".to_string()
        };

        let footer_text = format!("{}\n{}\n{}", license_text, INFO_TEXT[0], INFO_TEXT[1]);

        let info_footer = Paragraph::new(Text::from(footer_text))
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
        assert!(INFO_TEXT[0].contains("move up"));
        assert!(INFO_TEXT[0].contains("move down"));
        assert!(INFO_TEXT[1].contains("restrictive"));
        assert!(INFO_TEXT[1].contains("incompatible"));
        assert!(INFO_TEXT[1].contains("compatible"));
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
}
