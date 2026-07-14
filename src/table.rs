use crate::debug::{log, log_debug, LogLevel};
use crate::licenses::{LicenseCompatibility, LicenseInfo};
use color_eyre::Result;
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Flex, Layout, Position, Rect},
    style::{self, Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Cell, HighlightSpacing, Padding, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Table, TableState, Wrap,
    },
    DefaultTerminal, Frame,
};
use style::palette::tailwind;
use unicode_width::UnicodeWidthStr;

const HELP_TEXT: [&str; 14] = [
    "Navigation",
    "  ↑/k  move up        ↓/j  move down",
    "  ←/h  column left    →/l  column right",
    "  Enter  package details",
    "",
    "Filters (toggle)",
    "  r  restrictive      i  incompatible     c  compatible",
    "  a  osi-approved     n  osi-not-approved u  osi-unknown",
    "  x  clear all filters",
    "",
    "Sorting",
    "  s  enter sort mode (←→ pick column, Enter apply/toggle, Esc exit)",
    "",
    "  ?  toggle this help    Esc/q  quit",
];

const ITEM_HEIGHT: usize = 1;

/// Caps applied to content-derived column widths so one long value
/// (e.g. a 131-char license expression) cannot starve the other columns.
const MAX_NAME_WIDTH: u16 = 35;
const MAX_VERSION_WIDTH: u16 = 20;
const MAX_LICENSE_WIDTH: u16 = 50;

// ============================================================================
// KEY BINDINGS CONFIGURATION
// ============================================================================
// All GUI key bindings for normal and sorting modes are centrally defined here.
// This makes it easy to view, manage, and modify keybindings in one place.

/// Normal mode key bindings
#[allow(dead_code)]
pub mod keybindings_normal {
    use ratatui::crossterm::event::KeyCode;

    /// Quit the application
    pub const QUIT: &[KeyCode] = &[KeyCode::Esc];
    pub const QUIT_CHAR: char = 'q';

    /// Navigation keys
    pub const MOVE_DOWN: &[KeyCode] = &[KeyCode::Down];
    pub const MOVE_DOWN_CHAR: char = 'j';

    pub const MOVE_UP: &[KeyCode] = &[KeyCode::Up];
    pub const MOVE_UP_CHAR: char = 'k';

    pub const MOVE_RIGHT: &[KeyCode] = &[KeyCode::Right];
    pub const MOVE_RIGHT_CHAR: char = 'l';

    pub const MOVE_LEFT: &[KeyCode] = &[KeyCode::Left];
    pub const MOVE_LEFT_CHAR: char = 'h';

    /// Filter keys
    pub const FILTER_RESTRICTIVE: char = 'r';
    pub const FILTER_INCOMPATIBLE: char = 'i';
    pub const FILTER_COMPATIBLE: char = 'c';
    pub const FILTER_OSI_APPROVED: char = 'a';
    pub const FILTER_OSI_NOT_APPROVED: char = 'n';
    pub const FILTER_OSI_UNKNOWN: char = 'u';
    pub const FILTER_CLEAR_ALL: char = 'x';

    /// Sort mode
    pub const ENTER_SORT_MODE: char = 's';

    /// Help overlay
    pub const TOGGLE_HELP: char = '?';

    /// Package detail popup
    pub const SHOW_DETAILS: KeyCode = KeyCode::Enter;
}

/// Sort mode key bindings
#[allow(dead_code)]
pub mod keybindings_sort {
    use ratatui::crossterm::event::KeyCode;

    /// Navigate between columns
    pub const SELECT_PREV_COLUMN: &[KeyCode] = &[KeyCode::Left];
    pub const SELECT_PREV_COLUMN_CHAR: char = 'h';

    pub const SELECT_NEXT_COLUMN: &[KeyCode] = &[KeyCode::Right];
    pub const SELECT_NEXT_COLUMN_CHAR: char = 'l';

    /// Apply sort
    pub const APPLY_SORT: KeyCode = KeyCode::Enter;

    /// Exit sort mode
    pub const EXIT_SORT_MODE: &[KeyCode] = &[KeyCode::Esc];
    pub const EXIT_SORT_MODE_CHAR: char = 'q';
}

const TABLE_COLOUR: tailwind::Palette = tailwind::BLUE;

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
    dim_fg: Color,
    accent: Color,
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
    restrictive_color: Color,
    non_restrictive_color: Color,
    glass_tint: Color,
    glass_sheen: Color,
    glass_border: Color,
}

impl TableColors {
    const fn new(color: &tailwind::Palette) -> Self {
        Self {
            buffer_bg: Color::Rgb(0, 0, 0),
            header_bg: tailwind::SLATE.c800,
            header_fg: tailwind::SLATE.c100,
            row_fg: tailwind::SLATE.c200,
            dim_fg: tailwind::SLATE.c400,
            accent: color.c400,
            selected_row_style_fg: color.c400,
            selected_column_style_fg: color.c400,
            selected_cell_style_fg: color.c600,
            normal_row_color: Color::Rgb(0, 0, 0),
            alt_row_color: tailwind::SLATE.c950,
            footer_border_color: color.c400,
            compatible_color: tailwind::GREEN.c500,
            incompatible_color: tailwind::RED.c500,
            unknown_color: tailwind::YELLOW.c500,
            osi_approved_color: tailwind::BLUE.c500,
            osi_not_approved_color: tailwind::ORANGE.c500,
            osi_unknown_color: tailwind::GRAY.c500,
            restrictive_color: tailwind::RED.c500,
            non_restrictive_color: tailwind::SLATE.c500,
            glass_tint: tailwind::SLATE.c900,
            glass_sheen: tailwind::SLATE.c700,
            glass_border: tailwind::SLATE.c400,
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
    filters: FilterState,
    sort_column: Option<SortColumn>,
    sort_direction: SortDirection,
    mode: AppMode,
    sort_column_selection: usize, // Index in SortColumn::all()
    show_help: bool,
    show_detail: bool,
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
            sort_column: None,
            sort_direction: SortDirection::Ascending,
            mode: AppMode::Normal,
            sort_column_selection: 0,
            show_help: false,
            show_detail: false,
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
                    // Popups swallow input until dismissed
                    if self.show_help {
                        if matches!(
                            key.code,
                            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?')
                        ) {
                            self.show_help = false;
                        }
                        continue;
                    }
                    if self.show_detail {
                        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter) {
                            self.show_detail = false;
                        }
                        continue;
                    }

                    match self.mode {
                        AppMode::Normal => match key.code {
                            // Popups
                            KeyCode::Char(c) if c == keybindings_normal::TOGGLE_HELP => {
                                self.show_help = true;
                            }
                            KeyCode::Enter => {
                                if !self.get_filtered_items().is_empty() {
                                    self.show_detail = true;
                                }
                            }
                            // Quit
                            KeyCode::Esc => {
                                log(LogLevel::Info, "Quitting TUI application");
                                return Ok(());
                            }
                            KeyCode::Char(c) if c == keybindings_normal::QUIT_CHAR => {
                                log(LogLevel::Info, "Quitting TUI application");
                                return Ok(());
                            }
                            // Navigation
                            KeyCode::Down => self.next_row(),
                            KeyCode::Char(c) if c == keybindings_normal::MOVE_DOWN_CHAR => {
                                self.next_row()
                            }
                            KeyCode::Up => self.previous_row(),
                            KeyCode::Char(c) if c == keybindings_normal::MOVE_UP_CHAR => {
                                self.previous_row()
                            }
                            KeyCode::Right => self.next_column(),
                            KeyCode::Char(c) if c == keybindings_normal::MOVE_RIGHT_CHAR => {
                                self.next_column()
                            }
                            KeyCode::Left => self.previous_column(),
                            KeyCode::Char(c) if c == keybindings_normal::MOVE_LEFT_CHAR => {
                                self.previous_column()
                            }
                            // Filters
                            KeyCode::Char(c) if c == keybindings_normal::FILTER_RESTRICTIVE => {
                                self.toggle_restrictive_filter()
                            }
                            KeyCode::Char(c) if c == keybindings_normal::FILTER_INCOMPATIBLE => {
                                self.toggle_incompatible_filter()
                            }
                            KeyCode::Char(c) if c == keybindings_normal::FILTER_COMPATIBLE => {
                                self.toggle_compatible_filter()
                            }
                            KeyCode::Char(c) if c == keybindings_normal::FILTER_OSI_APPROVED => {
                                self.toggle_osi_approved_filter()
                            }
                            KeyCode::Char(c)
                                if c == keybindings_normal::FILTER_OSI_NOT_APPROVED =>
                            {
                                self.toggle_osi_not_approved_filter()
                            }
                            KeyCode::Char(c) if c == keybindings_normal::FILTER_OSI_UNKNOWN => {
                                self.toggle_osi_unknown_filter()
                            }
                            KeyCode::Char(c) if c == keybindings_normal::FILTER_CLEAR_ALL => {
                                self.clear_filters()
                            }
                            // Sort mode
                            KeyCode::Char(c) if c == keybindings_normal::ENTER_SORT_MODE => {
                                self.enter_sort_mode()
                            }
                            _ => {}
                        },
                        AppMode::Sorting => match key.code {
                            // Navigate columns
                            KeyCode::Left => self.previous_sort_column(),
                            KeyCode::Char(c) if c == keybindings_sort::SELECT_PREV_COLUMN_CHAR => {
                                self.previous_sort_column()
                            }
                            KeyCode::Right => self.next_sort_column(),
                            KeyCode::Char(c) if c == keybindings_sort::SELECT_NEXT_COLUMN_CHAR => {
                                self.next_sort_column()
                            }
                            // Apply sort
                            KeyCode::Enter => self.apply_current_sort(),
                            // Exit sort mode
                            KeyCode::Esc => self.exit_sort_mode(),
                            KeyCode::Char(c) if c == keybindings_sort::EXIT_SORT_MODE_CHAR => {
                                self.exit_sort_mode()
                            }
                            _ => {}
                        },
                    }
                }
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        self.set_colors();

        // Paint the whole frame background so gutters and bars blend in
        frame.render_widget(
            Block::default().style(Style::new().bg(self.colors.buffer_bg)),
            frame.area(),
        );

        // Add space for filter bar if filters are active
        let vertical = if self.filters.is_any_active() {
            Layout::vertical([
                Constraint::Length(1), // title
                Constraint::Length(3), // filter bar
                Constraint::Min(5),    // table
                Constraint::Length(1), // footer
            ])
        } else {
            Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(0),
                Constraint::Min(5),
                Constraint::Length(1),
            ])
        };
        let rects = vertical.split(frame.area());

        self.render_title(frame, rects[0]);
        if self.filters.is_any_active() {
            self.render_filter_bar(frame, rects[1]);
        }

        // Reserve the rightmost column of the table area as a scrollbar gutter
        let table_area = Rect {
            width: rects[2].width.saturating_sub(1),
            ..rects[2]
        };
        let gutter = Rect {
            x: rects[2].x + rects[2].width.saturating_sub(1),
            width: 1,
            ..rects[2]
        };
        self.render_table(frame, table_area);
        self.render_scrollbar(frame, gutter);
        self.render_footer(frame, rects[3]);

        if self.show_detail {
            self.render_detail_popup(frame);
        }
        if self.show_help {
            self.render_help_popup(frame);
        }
    }

    fn render_title(&self, frame: &mut Frame, area: Rect) {
        let restrictive_count = self.items.iter().filter(|i| i.is_restrictive).count();
        let license_text = match &self.project_license {
            Some(license) => license.clone(),
            None => "Unknown".to_string(),
        };

        let mut spans = vec![
            Span::styled(
                " Feluda ",
                Style::new()
                    .fg(self.colors.header_fg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("│ ", Style::new().fg(self.colors.dim_fg)),
            Span::styled("Project: ", Style::new().fg(self.colors.dim_fg)),
            Span::styled(license_text, Style::new().fg(self.colors.row_fg)),
            Span::styled("  │  ", Style::new().fg(self.colors.dim_fg)),
            Span::styled(
                format!("{} packages", self.items.len()),
                Style::new().fg(self.colors.row_fg),
            ),
        ];
        if restrictive_count > 0 {
            spans.push(Span::styled("  ·  ", Style::new().fg(self.colors.dim_fg)));
            spans.push(Span::styled(
                format!("{restrictive_count} restrictive"),
                Style::new().fg(self.colors.restrictive_color),
            ));
        }
        if let Some(column) = self.sort_column {
            let direction = match self.sort_direction {
                SortDirection::Ascending => "↑",
                SortDirection::Descending => "↓",
            };
            spans.push(Span::styled("  │  ", Style::new().fg(self.colors.dim_fg)));
            spans.push(Span::styled("Sort: ", Style::new().fg(self.colors.dim_fg)));
            spans.push(Span::styled(
                format!("{} {}", column.display_name(), direction),
                Style::new().fg(self.colors.accent),
            ));
        }

        let version = Line::from(Span::styled(
            concat!("v", env!("CARGO_PKG_VERSION"), " "),
            Style::new().fg(self.colors.dim_fg),
        ))
        .right_aligned();

        let title = Block::new().title(Line::from(spans)).title(version).style(
            Style::new()
                .fg(self.colors.row_fg)
                .bg(self.colors.header_bg),
        );
        frame.render_widget(title, area);
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

        // Add sort indicators to column headers if sorting is active.
        // In sort mode, the header cell under the cursor is highlighted.
        let header = SortColumn::all()
            .iter()
            .enumerate()
            .map(|(idx, col)| {
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

                let cell = Cell::from(display_name);
                if self.mode == AppMode::Sorting && idx == self.sort_column_selection {
                    cell.style(
                        Style::new()
                            .fg(self.colors.buffer_bg)
                            .bg(self.colors.accent)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    cell
                }
            })
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
                    Text::from("Compatible").fg(self.colors.compatible_color)
                }
                LicenseCompatibility::Incompatible => {
                    Text::from("Incompatible").fg(self.colors.incompatible_color)
                }
                LicenseCompatibility::Unknown => {
                    Text::from("Unknown").fg(self.colors.unknown_color)
                }
            };

            // Style OSI status text based on its value
            let osi_status_text = match data.osi_status {
                crate::licenses::OsiStatus::Approved => {
                    Text::from("approved").fg(self.colors.osi_approved_color)
                }
                crate::licenses::OsiStatus::NotApproved => {
                    Text::from("not-approved").fg(self.colors.osi_not_approved_color)
                }
                crate::licenses::OsiStatus::Unknown => {
                    Text::from("unknown").fg(self.colors.osi_unknown_color)
                }
            };

            let restrictive_text = if data.is_restrictive {
                Text::from("Yes").fg(self.colors.restrictive_color)
            } else {
                Text::from("No").fg(self.colors.non_restrictive_color)
            };

            Row::new([
                Cell::from(Text::from(truncate_with_ellipsis(
                    &data.name,
                    MAX_NAME_WIDTH,
                ))),
                Cell::from(Text::from(truncate_with_ellipsis(
                    &data.version,
                    MAX_VERSION_WIDTH,
                ))),
                Cell::from(Text::from(truncate_with_ellipsis(
                    &data.get_license(),
                    MAX_LICENSE_WIDTH,
                ))),
                Cell::from(restrictive_text),
                Cell::from(compatibility_text),
                Cell::from(osi_status_text),
            ])
            .style(Style::new().fg(self.colors.row_fg).bg(color))
            .height(ITEM_HEIGHT as u16)
        });

        let t = Table::new(
            rows,
            [
                // Name shrinks last: everything else is fixed-width, so when
                // the terminal is narrow the Min column gives way gracefully
                // instead of the layout dropping a column entirely.
                Constraint::Min(self.longest_item_lens.0 + 1),
                Constraint::Length(self.longest_item_lens.1 + 1),
                Constraint::Length(self.longest_item_lens.2 + 1),
                Constraint::Length(self.longest_item_lens.3),
                Constraint::Length(self.longest_item_lens.4), // Compatibility column
                Constraint::Length(self.longest_item_lens.5), // OSI Status column
            ],
        )
        .header(header)
        .row_highlight_style(selected_row_style)
        .column_highlight_style(selected_col_style)
        .cell_highlight_style(selected_cell_style)
        .highlight_symbol(" █ ")
        .bg(self.colors.buffer_bg)
        .highlight_spacing(HighlightSpacing::Always);

        frame.render_stateful_widget(t, area, &mut self.state);

        log(
            LogLevel::Info,
            &format!(
                "Table rendered with {filtered_count} rows (filtered from {total_count} total)"
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
        // Skip the header line so the track aligns with the data rows
        let track = Rect {
            y: area.y + 1,
            height: area.height.saturating_sub(1),
            ..area
        };
        frame.render_stateful_widget(
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .style(Style::new().fg(self.colors.dim_fg))
                .thumb_style(Style::new().fg(self.colors.accent))
                .begin_symbol(None)
                .end_symbol(None),
            track,
            &mut self.scroll_state,
        );
    }

    /// Build a "key label" hint pair for the footer
    fn key_hint(&self, key: &str, label: &str) -> [Span<'static>; 2] {
        [
            Span::styled(
                format!(" {key} "),
                Style::new()
                    .fg(self.colors.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{label}  "), Style::new().fg(self.colors.dim_fg)),
        ]
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let hints: Vec<(&str, &str)> = if self.mode == AppMode::Sorting {
            vec![
                ("←→", "pick column"),
                ("Enter", "apply / toggle direction"),
                ("Esc", "cancel"),
            ]
        } else {
            vec![
                ("↑↓", "move"),
                ("Enter", "details"),
                ("s", "sort"),
                ("r/i/c/a/n/u", "filter"),
                ("x", "clear"),
                ("?", "help"),
                ("q", "quit"),
            ]
        };

        let mut spans = Vec::with_capacity(hints.len() * 2 + 1);
        if self.mode == AppMode::Sorting {
            spans.push(Span::styled(
                " SORT ",
                Style::new()
                    .fg(self.colors.buffer_bg)
                    .bg(self.colors.accent)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        for (key, label) in hints {
            spans.extend(self.key_hint(key, label));
        }

        let footer = Paragraph::new(Line::from(spans)).style(
            Style::new()
                .fg(self.colors.row_fg)
                .bg(self.colors.alt_row_color),
        );
        frame.render_widget(footer, area);
    }

    /// Centered popup rect of at most `width` x `height` within the frame
    fn popup_area(frame: &Frame, width: u16, height: u16) -> Rect {
        let [area] = Layout::horizontal([Constraint::Length(width)])
            .flex(Flex::Center)
            .areas(frame.area());
        let [area] = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::Center)
            .areas(area);
        area
    }

    /// Imitate depth-of-field behind a modal: everything outside `focus`
    /// fades toward black, so the popup reads as the only sharp layer.
    fn render_scrim(frame: &mut Frame, focus: Rect) {
        let area = frame.area();
        let buf = frame.buffer_mut();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if focus.contains(Position { x, y }) {
                    continue;
                }
                if let Some(cell) = buf.cell_mut(Position { x, y }) {
                    cell.fg = blend(cell.fg, Color::Rgb(0, 0, 0), 0.8);
                    cell.bg = blend(cell.bg, Color::Rgb(0, 0, 0), 0.8);
                }
            }
        }
    }

    /// Frosted-glass panel: instead of clearing what's underneath, wash it
    /// toward the tint so the table shimmers through faintly, with a lighter
    /// sheen at the top edge like light catching glass.
    fn render_frost(&self, frame: &mut Frame, area: Rect) {
        let buf = frame.buffer_mut();
        for y in area.top()..area.bottom() {
            let depth = f32::from(y.saturating_sub(area.top())) / f32::from(area.height.max(1));
            let tint = blend(
                self.colors.glass_tint,
                self.colors.glass_sheen,
                0.45 * (1.0 - depth),
            );
            for x in area.left()..area.right() {
                if let Some(cell) = buf.cell_mut(Position { x, y }) {
                    cell.bg = blend(cell.bg, tint, 0.88);
                    // Old glyphs stay as the faintest texture; anything more
                    // visible competes with the card's own text
                    cell.fg = blend(cell.fg, tint, 0.95);
                    // A reversed or bold cell under the glass would punch
                    // through the effect (and invert card text drawn over it)
                    cell.modifier = Modifier::empty();
                }
            }
        }
    }

    /// Scrim + frost + bordered card. The paragraph carries no background of
    /// its own, so the frosted cells stay visible in the padding and between
    /// spans, which is what sells the translucency.
    fn render_glass_card(&self, frame: &mut Frame, area: Rect, title: &str, lines: Vec<Line>) {
        Self::render_scrim(frame, area);
        self.render_frost(frame, area);
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }).block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(self.colors.glass_border))
                    .padding(Padding::new(2, 2, 1, 1))
                    .title(Span::styled(
                        format!(" {title} "),
                        Style::new()
                            .fg(self.colors.header_fg)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .title_bottom(
                        Line::from(Span::styled(
                            " (Esc) close ",
                            Style::new().fg(self.colors.dim_fg),
                        ))
                        .right_aligned(),
                    ),
            ),
            area,
        );
    }

    fn render_help_popup(&self, frame: &mut Frame) {
        let width = (HELP_TEXT.iter().map(|l| l.width()).max().unwrap_or(0) as u16 + 8)
            .min(frame.area().width.saturating_sub(4));
        let height = (HELP_TEXT.len() as u16 + 4).min(frame.area().height.saturating_sub(2));
        let area = Self::popup_area(frame, width, height);

        let lines: Vec<Line> = HELP_TEXT
            .iter()
            .map(|l| Line::from(*l).fg(self.colors.row_fg))
            .collect();

        self.render_glass_card(frame, area, "Help", lines);
    }

    fn render_detail_popup(&self, frame: &mut Frame) {
        let filtered_items = self.get_filtered_items();
        let Some(selected) = self.state.selected() else {
            return;
        };
        let Some(item) = filtered_items.get(selected).copied() else {
            return;
        };

        let label_style = Style::new().fg(self.colors.dim_fg);
        let value_style = Style::new().fg(self.colors.row_fg);

        // Status chips: colored dot + short verdict
        let compatibility_chip = match item.compatibility {
            LicenseCompatibility::Compatible => (
                self.colors.compatible_color,
                match &self.project_license {
                    Some(license) => format!("Compatible with {license}"),
                    None => "Compatible".to_string(),
                },
            ),
            LicenseCompatibility::Incompatible => (
                self.colors.incompatible_color,
                match &self.project_license {
                    Some(license) => format!("Incompatible with {license}"),
                    None => "Incompatible".to_string(),
                },
            ),
            LicenseCompatibility::Unknown => (
                self.colors.unknown_color,
                "Unknown compatibility".to_string(),
            ),
        };
        let osi_chip = match item.osi_status {
            crate::licenses::OsiStatus::Approved => {
                (self.colors.osi_approved_color, "OSI approved")
            }
            crate::licenses::OsiStatus::NotApproved => {
                (self.colors.osi_not_approved_color, "Not OSI approved")
            }
            crate::licenses::OsiStatus::Unknown => {
                (self.colors.osi_unknown_color, "OSI status unknown")
            }
        };
        let restrictive_chip = if item.is_restrictive {
            (self.colors.restrictive_color, "Restrictive")
        } else {
            (self.colors.non_restrictive_color, "Not restrictive")
        };

        let chip = |(color, text): (Color, String)| -> Vec<Span<'static>> {
            vec![
                Span::styled("● ", Style::new().fg(color)),
                Span::styled(text, Style::new().fg(color)),
                Span::raw("   "),
            ]
        };

        let mut chips_line = Vec::new();
        chips_line.extend(chip(compatibility_chip));
        chips_line.extend(chip((osi_chip.0, osi_chip.1.to_string())));
        chips_line.extend(chip((restrictive_chip.0, restrictive_chip.1.to_string())));

        // How common is this exact license expression in the project?
        let same_license_count = self
            .items
            .iter()
            .filter(|other| other.get_license() == item.get_license())
            .count()
            .saturating_sub(1);
        let shared_text = if same_license_count == 0 {
            "no other package in this project".to_string()
        } else if same_license_count == 1 {
            "1 other package in this project".to_string()
        } else {
            format!("{same_license_count} other packages in this project")
        };

        let position_text = if self.filters.is_any_active() {
            format!(
                "{} of {} shown ({} total)",
                selected + 1,
                filtered_items.len(),
                self.items.len()
            )
        } else {
            format!("{} of {}", selected + 1, self.items.len())
        };

        let mut lines = vec![
            Line::from(vec![
                Span::styled(item.name.clone(), value_style.add_modifier(Modifier::BOLD)),
                Span::styled(format!("  v{}", item.version), label_style),
            ]),
            Line::from(chips_line),
            Line::raw(""),
            Line::from(Span::styled("License", label_style)),
            Line::from(Span::styled(item.get_license(), value_style)),
            Line::raw(""),
        ];
        if let Some(ref sub_project) = item.sub_project {
            lines.push(Line::from(vec![
                Span::styled("Sub-project    ", label_style),
                Span::styled(sub_project.clone(), value_style),
            ]));
        }
        lines.push(Line::from(vec![
            Span::styled("Same license   ", label_style),
            Span::styled(shared_text, value_style),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Package        ", label_style),
            Span::styled(position_text, value_style),
        ]));

        let width = 76.min(frame.area().width.saturating_sub(4));
        // Long license expressions wrap; leave room for the extra lines
        let inner_width = width.saturating_sub(6).max(1);
        let license_extra = (item.get_license().width() as u16) / inner_width;
        let height =
            (lines.len() as u16 + 4 + license_extra).min(frame.area().height.saturating_sub(2));
        let area = Self::popup_area(frame, width, height);

        self.render_glass_card(frame, area, "Package Details", lines);
    }
}

/// RGB components of a color; non-RGB colors (Reset etc.) fall back to the
/// app background so blending degrades gracefully.
fn rgb_components(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0), // the app background
    }
}

/// Linear blend between two colors; `t` = 0.0 keeps `from`, 1.0 gives `to`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn blend(from: Color, to: Color, t: f32) -> Color {
    let (fr, fg, fb) = rgb_components(from);
    let (tr, tg, tb) = rgb_components(to);
    let mix =
        |f: u8, to_c: u8| -> u8 { (f32::from(f) + (f32::from(to_c) - f32::from(f)) * t) as u8 };
    Color::Rgb(mix(fr, tr), mix(fg, tg), mix(fb, tb))
}

/// Truncate a string to `max_width` display columns, appending an ellipsis
/// when content was cut off.
fn truncate_with_ellipsis(s: &str, max_width: u16) -> String {
    let max_width = max_width as usize;
    if s.width() <= max_width {
        return s.to_string();
    }
    let mut out = String::new();
    let mut width = 0;
    for c in s.chars() {
        let w = c.to_string().width();
        if width + w > max_width.saturating_sub(1) {
            break;
        }
        width += w;
        out.push(c);
    }
    out.push('…');
    out
}

fn constraint_len_calculator(items: &[LicenseInfo]) -> (u16, u16, u16, u16, u16, u16) {
    log(LogLevel::Info, "Calculating column widths for table");

    // Each column must fit its header plus a possible sort arrow (" ↑"),
    // and content-driven widths are capped so one long value cannot
    // starve the other columns out of the layout.
    let header_len = |header: &str| header.width() + 2;

    let name_len = items
        .iter()
        .map(LicenseInfo::name)
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0)
        .max(header_len("Name"))
        .min(MAX_NAME_WIDTH as usize);

    let version_len = items
        .iter()
        .map(LicenseInfo::version)
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0)
        .max(header_len("Version"))
        .min(MAX_VERSION_WIDTH as usize);

    let license_len = items
        .iter()
        .map(|info| info.get_license())
        .map(|s| s.width())
        .max()
        .unwrap_or(0)
        .max(header_len("License"))
        .min(MAX_LICENSE_WIDTH as usize);

    let restricted_len = "Yes"
        .width()
        .max("No".width())
        .max(header_len("Restrictive"));

    // Calculate width for the Compatibility column
    let compatibility_len = ["Compatible", "Incompatible", "Unknown"]
        .iter()
        .map(|s| s.width())
        .max()
        .unwrap_or(0)
        .max(header_len("Compatibility"));

    // Calculate width for the OSI Status column
    let osi_status_len = ["approved", "not-approved", "unknown"]
        .iter()
        .map(|s| s.width())
        .max()
        .unwrap_or(0)
        .max(header_len("OSI Status"));

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
            sub_project: None,
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
                sub_project: None,
            },
            LicenseInfo {
                name: "package2".to_string(),
                version: "2.0.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
            },
            LicenseInfo {
                name: "package3".to_string(),
                version: "3.0.0".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
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
            sub_project: None,
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
                sub_project: None,
            },
            LicenseInfo {
                name: "short".to_string(),
                version: "2.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
            },
        ];

        let (name_len, version_len, license_len, restricted_len, compatibility_len, _osi_len) =
            constraint_len_calculator(&test_data);

        // Content longer than the caps is clamped
        assert_eq!(name_len, MAX_NAME_WIDTH);
        assert_eq!(version_len, MAX_VERSION_WIDTH);
        assert_eq!(license_len, "Apache-2.0".len() as u16);
        // Fixed columns are sized to their headers plus sort-arrow room
        assert_eq!(restricted_len, "Restrictive".len() as u16 + 2);
        assert_eq!(compatibility_len, "Compatibility".len() as u16 + 2);
    }

    #[test]
    fn test_constraint_len_calculator_empty() {
        let test_data = vec![];
        let (name_len, version_len, license_len, restricted_len, compatibility_len, _osi_len) =
            constraint_len_calculator(&test_data);

        // With no items, columns still fit their headers plus sort-arrow room
        assert_eq!(name_len, "Name".len() as u16 + 2);
        assert_eq!(version_len, "Version".len() as u16 + 2);
        assert_eq!(license_len, "License".len() as u16 + 2);
        assert_eq!(restricted_len, "Restrictive".len() as u16 + 2);
        assert_eq!(compatibility_len, "Compatibility".len() as u16 + 2);
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
            sub_project: None,
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
                sub_project: None,
            },
            LicenseInfo {
                name: "incompatible".to_string(),
                version: "1.0.0".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
            },
            LicenseInfo {
                name: "unknown".to_string(),
                version: "1.0.0".to_string(),
                license: Some("Custom".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Unknown,
                osi_status: crate::licenses::OsiStatus::Unknown,
                sub_project: None,
            },
        ];

        let (_, _, _, _, compatibility_len, _) = constraint_len_calculator(&test_data);

        assert_eq!(compatibility_len, "Compatibility".len() as u16 + 2);
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
                sub_project: None,
            },
            LicenseInfo {
                name: "package2".to_string(),
                version: "1.0.0".to_string(),
                license: Some("Apache".to_string()),
                is_restrictive: false, // false
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
            },
        ];

        let (_, _, _, restricted_len, _, _) = constraint_len_calculator(&test_data);

        assert_eq!(restricted_len, "Restrictive".len() as u16 + 2);
    }

    #[test]
    fn test_item_height_constant() {
        assert_eq!(ITEM_HEIGHT, 1);
    }

    #[test]
    fn test_help_text_constant() {
        let help = HELP_TEXT.join("\n");
        assert!(help.contains("quit"));
        assert!(help.contains("move up"));
        assert!(help.contains("move down"));
        assert!(help.contains("restrictive"));
        assert!(help.contains("incompatible"));
        assert!(help.contains("compatible"));
        assert!(help.contains("sort mode"));
        assert!(help.contains("Enter"));
        assert!(help.contains("details"));
    }

    #[test]
    fn test_truncate_with_ellipsis() {
        assert_eq!(truncate_with_ellipsis("short", 10), "short");
        assert_eq!(truncate_with_ellipsis("exactly-10", 10), "exactly-10");
        assert_eq!(
            truncate_with_ellipsis("this is far too long", 10),
            "this is f…"
        );
        assert_eq!(truncate_with_ellipsis("", 10), "");
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
                sub_project: None,
            },
            LicenseInfo {
                name: "much_longer_name".to_string(),
                version: "1.0.0-beta".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
            },
        ];

        let app = App::new(test_data, None);

        assert_eq!(app.longest_item_lens.0, "much_longer_name".len() as u16);
        assert_eq!(app.longest_item_lens.1, "1.0.0-beta".len() as u16);
        assert_eq!(app.longest_item_lens.2, "Apache-2.0".len() as u16);
        assert_eq!(app.longest_item_lens.3, "Restrictive".len() as u16 + 2);
        assert_eq!(app.longest_item_lens.4, "Compatibility".len() as u16 + 2);
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
                sub_project: None,
            },
            LicenseInfo {
                name: "apple".to_string(),
                version: "2.0.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
            },
            LicenseInfo {
                name: "banana".to_string(),
                version: "3.0.0".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
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
                sub_project: None,
            },
            LicenseInfo {
                name: "zebra".to_string(),
                version: "2.0.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
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
                sub_project: None,
            },
            LicenseInfo {
                name: "package2".to_string(),
                version: "2.0.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
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
            sub_project: None,
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
            sub_project: None,
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
                sub_project: None,
            },
            LicenseInfo {
                name: "apple".to_string(),
                version: "5.0.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
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
            sub_project: None,
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
                sub_project: None,
            },
            LicenseInfo {
                name: "package2".to_string(),
                version: "v1.0.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
            },
            LicenseInfo {
                name: "package3".to_string(),
                version: "v2.5.0".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
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
                sub_project: None,
            },
            LicenseInfo {
                name: "package2".to_string(),
                version: "v1.5.0".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
            },
            LicenseInfo {
                name: "package3".to_string(),
                version: "v2.0.0".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
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
                sub_project: None,
            },
            LicenseInfo {
                name: "package2".to_string(),
                version: "0.14".to_string(),
                license: Some("Apache-2.0".to_string()),
                is_restrictive: false,
                compatibility: LicenseCompatibility::Compatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
            },
            LicenseInfo {
                name: "package3".to_string(),
                version: "2015.7".to_string(),
                license: Some("GPL-3.0".to_string()),
                is_restrictive: true,
                compatibility: LicenseCompatibility::Incompatible,
                osi_status: crate::licenses::OsiStatus::Approved,
                sub_project: None,
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
