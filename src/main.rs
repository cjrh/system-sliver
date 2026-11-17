//! A system monitor designed for a narrow, screen-edge window.
mod chart;
mod metrics;
mod zoom;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chart::{Chart, Metric};
use iced::widget::{Space, button, canvas, column, container, responsive, row, scrollable, text};
use iced::{Alignment, Color, Element, Length, Size, Subscription, Task, window};
use iced_themer::{ThemeConfig, Themed};
use metrics::{History, Sample, Sampler, bytes, percent};

const SIZE_TITLE: f32 = 15.0;
const SIZE_VALUE: f32 = 30.0;
const SIZE_BODY: f32 = 14.0;
const SIZE_SMALL: f32 = 12.0;
const SIZE_CAPTION: f32 = 10.0;

const UPDATE_INTERVALS: &[(u64, &str)] = &[(1, "1s"), (3, "3s"), (10, "10s"), (60, "1m")];
const HISTORY_RANGES: &[(u64, &str)] = &[(60, "1m"), (300, "5m"), (900, "15m"), (3600, "1h")];

fn main() -> iced::Result {
    prefer_integrated_gpu();
    let theme = Arc::new(
        include_str!("../theme.toml")
            .parse::<ThemeConfig>()
            .expect("valid embedded theme"),
    );
    let boot_theme = theme.clone();
    iced::application(move || boot(boot_theme.clone()), update, view)
        .title("System Sliver")
        .theme(move |_: &State| theme.theme())
        .window(window::Settings {
            size: Size::new(220.0, 820.0),
            min_size: Some(Size::new(120.0, 280.0)),
            platform_specific: window::settings::PlatformSpecific {
                application_id: "system-sliver".into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .subscription(subscription)
        .scale_factor(|state: &State| state.zoom.scale_factor())
        .run()
}

fn prefer_integrated_gpu() {
    if std::env::var_os("WGPU_POWER_PREF").is_none() {
        // SAFETY: first call in main, before any threads are started.
        unsafe { std::env::set_var("WGPU_POWER_PREF", "low") };
    }
}

struct State {
    theme: Arc<ThemeConfig>,
    zoom: zoom::Zoom,
    history: History,
    seconds: u64,
    interval_seconds: u64,
    sampler: Arc<Mutex<Option<Sampler>>>,
    busy: bool,
    error: Option<String>,
}

#[derive(Debug, Clone)]
enum Msg {
    Tick,
    Sampled(Result<Sample, String>),
    Range(u64),
    Interval(u64),
    Zoom(zoom::Action),
}

fn boot(theme: Arc<ThemeConfig>) -> (State, Task<Msg>) {
    let state = State {
        theme,
        zoom: zoom::Zoom::default(),
        history: History::default(),
        seconds: 60,
        interval_seconds: 1,
        sampler: Arc::new(Mutex::new(None)),
        busy: true,
        error: None,
    };
    let task = sample_task(state.sampler.clone(), state.interval_seconds);
    (state, task)
}

fn sample_task(sampler: Arc<Mutex<Option<Sampler>>>, interval_seconds: u64) -> Task<Msg> {
    Task::perform(
        async move {
            tokio::task::spawn_blocking(move || {
                let mut guard = sampler
                    .lock()
                    .map_err(|_| "Sampler lock poisoned".to_string())?;
                Ok(guard
                    .get_or_insert_with(Sampler::new)
                    .sample(Duration::from_secs(interval_seconds)))
            })
            .await
            .map_err(|e| format!("Sampler failed: {e}"))?
        },
        Msg::Sampled,
    )
}

fn subscription(state: &State) -> Subscription<Msg> {
    Subscription::batch([
        iced::time::every(Duration::from_secs(state.interval_seconds)).map(|_| Msg::Tick),
        iced::event::listen_with(|event, _, _| match event {
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                zoom::shortcut(&key, modifiers).map(Msg::Zoom)
            }
            _ => None,
        }),
    ])
}

fn update(state: &mut State, message: Msg) -> Task<Msg> {
    match message {
        Msg::Tick if !state.busy => {
            state.busy = true;
            return sample_task(state.sampler.clone(), state.interval_seconds);
        }
        Msg::Sampled(result) => {
            state.busy = false;
            match result {
                Ok(sample) => {
                    state.history.push(sample);
                    state.error = None;
                }
                Err(error) => {
                    state.error = Some(error);
                }
            }
        }
        Msg::Range(seconds) => {
            if HISTORY_RANGES.iter().any(|&(value, _)| value == seconds) {
                state.seconds = seconds;
            }
        }
        Msg::Interval(seconds) => {
            if UPDATE_INTERVALS.iter().any(|&(value, _)| value == seconds) {
                state.interval_seconds = seconds;
            }
        }
        Msg::Zoom(action) => state.zoom.apply(action),
        Msg::Tick => {}
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Msg> {
    responsive(move |size| content(state, size.width)).into()
}

fn content(state: &State, width: f32) -> Element<'_, Msg> {
    let palette = state.theme.theme().palette();
    let muted = dim(palette.text, 0.45);
    let status = if state.error.is_some() {
        "ERROR"
    } else if state.history.0.len() < 2 {
        "WAIT"
    } else {
        "LIVE"
    };
    let status_color = if state.error.is_some() {
        palette.danger
    } else {
        palette.success
    };
    let header = row![
        text("SLIVER").size(SIZE_TITLE),
        Space::new().width(Length::Fill),
        text(status).size(SIZE_CAPTION).color(status_color),
    ]
    .align_y(Alignment::Center);
    let intervals = selector(UPDATE_INTERVALS, state.interval_seconds, Msg::Interval);
    let ranges = selector(HISTORY_RANGES, state.seconds, Msg::Range);
    let mut body = column![header, intervals, ranges].spacing(10);
    if let Some(error) = &state.error {
        body = body.push(text(error).size(SIZE_SMALL).color(palette.danger));
    }
    for metric in [Metric::Cpu, Metric::Memory, Metric::Swap, Metric::Network] {
        body = body.push(metric_card(state, metric, width < 175.0));
    }
    body = body
        .push(row![
            text(format!("−{}", duration_label(state.seconds)))
                .size(SIZE_CAPTION)
                .color(muted),
            Space::new().width(Length::Fill),
            text("now").size(SIZE_CAPTION).color(muted),
        ])
        .push(
            text(format!(
                "{} samples · Ctrl +/− zoom\nCtrl 0 reset",
                duration_label(state.interval_seconds)
            ))
            .size(SIZE_CAPTION)
            .color(muted),
        );
    // Reserve a slim scrollbar gutter instead of painting over the latest
    // values. Horizontal scrolling is only needed at extreme keyboard zoom.
    let scrollbar = iced::widget::scrollable::Scrollbar::new()
        .width(3)
        .scroller_width(3)
        .spacing(4);
    let (body_width, direction) = if width >= 120.0 {
        (
            Length::Fill,
            iced::widget::scrollable::Direction::Vertical(scrollbar),
        )
    } else {
        (
            Length::Fixed(100.0),
            iced::widget::scrollable::Direction::Both {
                vertical: scrollbar,
                horizontal: scrollbar,
            },
        )
    };
    container(
        scrollable(body.width(body_width))
            .direction(direction)
            .width(Length::Fill)
            .height(Length::Fill),
    )
    .padding(10)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn selector(
    options: &'static [(u64, &'static str)],
    selected: u64,
    on_select: fn(u64) -> Msg,
) -> Element<'static, Msg> {
    options
        .iter()
        .fold(row![].spacing(3), |row, &(seconds, label)| {
            row.push(
                button(text(label).size(SIZE_SMALL))
                    .padding([4, 0])
                    .width(Length::Fill)
                    .style(if seconds == selected {
                        button::primary
                    } else {
                        button::text
                    })
                    .on_press(on_select(seconds)),
            )
        })
        .into()
}

fn duration_label(seconds: u64) -> String {
    if seconds >= 3600 {
        format!("{}h", seconds / 3600)
    } else if seconds >= 60 {
        format!("{}m", seconds / 60)
    } else {
        format!("{seconds}s")
    }
}

fn metric_card(state: &State, metric: Metric, compact: bool) -> Element<'_, Msg> {
    let palette = state.theme.theme().palette();
    let (title, accent) = match metric {
        Metric::Cpu => ("CPU", palette.primary),
        Metric::Memory => ("MEMORY", palette.success),
        Metric::Swap => ("SWAP", palette.warning),
        Metric::Network => ("NETWORK", palette.primary),
    };
    let graph = Chart {
        history: &state.history,
        metric,
        seconds: state.seconds,
        color: accent,
        secondary: palette.warning,
        grid: dim(palette.text, 0.07),
    };
    let ceiling = graph.ceiling();
    let last = state.history.0.back();
    let (value, detail) = last.map_or_else(
        || ("—".into(), "Collecting…".into()),
        |s| match metric {
            Metric::Cpu => (
                s.cpu.map_or("—".into(), |cpu| format!("{cpu:.0}%")),
                format!("{} logical CPUs", s.cores),
            ),
            Metric::Memory => (
                format!("{:.0}%", percent(s.memory_used, s.memory_total)),
                format!(
                    "{} / {}",
                    bytes(s.memory_used as f32),
                    bytes(s.memory_total as f32)
                ),
            ),
            Metric::Swap => (
                if s.swap_total == 0 {
                    "—".into()
                } else {
                    format!("{:.0}%", percent(s.swap_used, s.swap_total))
                },
                if s.swap_total == 0 {
                    "No swap configured".into()
                } else {
                    format!(
                        "{} / {}",
                        bytes(s.swap_used as f32),
                        bytes(s.swap_total as f32)
                    )
                },
            ),
            Metric::Network => (String::new(), format!("Top {} /s", bytes(ceiling))),
        },
    );
    let mut card = column![text(title).size(SIZE_CAPTION).color(accent)].spacing(4);
    if matches!(metric, Metric::Network) {
        for (label, value, color) in [
            ("↓", last.and_then(|s| s.download), palette.primary),
            ("↑", last.and_then(|s| s.upload), palette.warning),
        ] {
            card = card.push(
                text(format!(
                    "{label} {}",
                    value.map_or("—".into(), |v| format!("{}/s", bytes(v)))
                ))
                .size(if compact { SIZE_SMALL } else { SIZE_BODY })
                .color(color),
            );
        }
    } else {
        card = card.push(text(value).size(if compact { 24.0 } else { SIZE_VALUE }));
    }
    card = card.push(
        text(detail)
            .size(SIZE_CAPTION)
            .color(dim(palette.text, 0.5)),
    );
    if matches!(metric, Metric::Memory) {
        let (low, high) = graph.range();
        card = card.push(
            text(format!("Plot {low:.1}–{high:.1}%"))
                .size(SIZE_CAPTION)
                .color(dim(palette.text, 0.5)),
        );
    }
    card = card.push(canvas(graph).width(Length::Fill).height(72));
    container(card)
        .padding(10)
        .width(Length::Fill)
        .themed(state.theme.container())
        .into()
}

fn dim(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_theme_parses() {
        assert!(include_str!("../theme.toml").parse::<ThemeConfig>().is_ok());
    }

    #[test]
    fn range_change_preserves_collected_history() {
        let theme = Arc::new(include_str!("../theme.toml").parse().unwrap());
        let (mut state, _) = boot(theme);
        state.history.push(Sample::fixture());
        let _ = update(&mut state, Msg::Range(300));
        assert_eq!(state.seconds, 300);
        assert_eq!(state.history.0.len(), 1);
    }

    #[test]
    fn invalid_controls_do_not_change_settings() {
        let theme = Arc::new(include_str!("../theme.toml").parse().unwrap());
        let (mut state, _) = boot(theme);
        for seconds in [0, 2, u64::MAX] {
            let _ = update(&mut state, Msg::Interval(seconds));
            let _ = update(&mut state, Msg::Range(seconds));
            assert_eq!(state.interval_seconds, 1);
            assert_eq!(state.seconds, 60);
        }
    }

    #[test]
    fn sampling_errors_preserve_history_and_recover() {
        let theme = Arc::new(include_str!("../theme.toml").parse().unwrap());
        let (mut state, _) = boot(theme);
        state.history.push(Sample::fixture());
        let _ = update(&mut state, Msg::Sampled(Err("test failure".into())));
        assert!(!state.busy);
        assert_eq!(state.error.as_deref(), Some("test failure"));
        assert_eq!(state.history.0.len(), 1);
        let _ = update(&mut state, Msg::Tick);
        assert!(state.busy);
        let _ = update(&mut state, Msg::Sampled(Ok(Sample::fixture())));
        assert!(!state.busy);
        assert!(state.error.is_none());
        assert_eq!(state.history.0.len(), 2);
    }

    #[test]
    fn interval_changes_preserve_history_and_range() {
        let theme = Arc::new(include_str!("../theme.toml").parse().unwrap());
        let (mut state, _) = boot(theme);
        state.history.push(Sample::fixture());
        let _ = update(&mut state, Msg::Range(3600));
        for &(seconds, label) in UPDATE_INTERVALS {
            let _ = update(&mut state, Msg::Interval(seconds));
            assert_eq!(state.interval_seconds, seconds);
            assert_eq!(duration_label(seconds), label);
            assert_eq!(state.seconds, 3600);
            assert_eq!(state.history.0.len(), 1);
        }
        assert_eq!(duration_label(state.seconds), "1h");
    }
}
