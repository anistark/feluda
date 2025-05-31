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

const INFO_TEXT: [&str; 1] =
    ["(Esc) quit | (↑) move up | (↓) move down | (←) move left | (→) move right"];

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
        }
    }
}

pub struct App {
    state: TableState,
    items: Vec<LicenseInfo>,
    longest_item_lens: (u16, u16, u16, u16, u16),
    scroll_state: ScrollbarState,
    colors: TableColors,
    project_license: Option<String>,
}

impl App {
    pub fn new(license_data: Vec<LicenseInfo>, project_license: Option<String>) -> Self {
        log(LogLevel::Info, "Initializing TUI application");
        log_debug("License data for TUI", &license_data);
        log(
            LogLevel::Info,
            format!("Project license: {:?}", project_license),
        );

        let data_vec = license_data;
        Self {
            state: TableState::default().with_selected(0),
            longest_item_lens: constraint_len_calculator(&data_vec),
            scroll_state: ScrollbarState::new((data_vec.len().saturating_sub(1)) * ITEM_HEIGHT),
            colors: TableColors::new(&TABLE_COLOUR),
            items: data_vec,
            project_license,
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
        log(LogLevel::Info, format!("Selected row: {}", i));
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
        log(LogLevel::Info, format!("Selected row: {}", i));
    }

    pub fn next_column(&mut self) {
        self.state.select_next_column();
        log(LogLevel::Info, "Selected next column");
    }

    pub fn previous_column(&mut self) {
        self.state.select_previous_column();
        log(LogLevel::Info, "Selected previous column");
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
                        _ => {}
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

        // Add Compatibility column to header
        let header = ["Name", "Version", "License", "Restrictive", "Compatibility"]
            .into_iter()
            .map(Cell::from)
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

            let row = Row::new([
                Cell::from(Text::from(format!("\n{}\n", data.name))),
                Cell::from(Text::from(format!("\n{}\n", data.version))),
                Cell::from(Text::from(format!("\n{}\n", data.get_license()))),
                Cell::from(Text::from(format!("\n{}\n", data.is_restrictive()))),
                Cell::from(compatibility_text),
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
                Constraint::Min(self.longest_item_lens.4), // Add constraint for Compatibility column
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
            format!("Table rendered with {} rows", self.items.len()),
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
        // Add project license information to footer if available
        let footer_text = if let Some(ref license) = self.project_license {
            format!("Project License: {} | {}", license, INFO_TEXT[0])
        } else {
            format!("Project License: Unknown | {}", INFO_TEXT[0])
        };

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

fn constraint_len_calculator(items: &[LicenseInfo]) -> (u16, u16, u16, u16, u16) {
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

    // Calculate width for the new Compatibility column
    let compatibility_len = ["Compatible", "Incompatible", "Unknown"]
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
    );

    log(
        LogLevel::Info,
        format!("Table column widths: {:?}", result),
    );
    result
}
