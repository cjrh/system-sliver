//! Small time-based plots. Missing history stays empty, not fabricated as zero.
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::{Color, Point, Rectangle, Renderer, Theme, mouse};

use crate::metrics::{History, Sample, percent};

#[derive(Clone, Copy)]
pub enum Metric {
    Cpu,
    Memory,
    Swap,
    Network,
}

pub struct Chart<'a> {
    pub history: &'a History,
    pub metric: Metric,
    pub seconds: u64,
    pub color: Color,
    pub secondary: Color,
    pub grid: Color,
}

impl Chart<'_> {
    pub fn ceiling(&self) -> f32 {
        if matches!(self.metric, Metric::Network) {
            self.history.0.back().map_or(1024.0, |last| {
                self.history
                    .0
                    .iter()
                    .filter(|s| {
                        last.at.saturating_duration_since(s.at).as_secs_f32() <= self.seconds as f32
                    })
                    .flat_map(|s| [s.download.unwrap_or(0.0), s.upload.unwrap_or(0.0)])
                    .fold(1024.0, f32::max)
                    * 1.1
            })
        } else {
            100.0
        }
    }

    /// Memory zooms into the visible range; other metrics retain their scales.
    pub fn range(&self) -> (f32, f32) {
        if !matches!(self.metric, Metric::Memory) {
            return (0.0, self.ceiling());
        }
        let Some(last) = self.history.0.back() else {
            return (0.0, 100.0);
        };
        let mut bounds: Option<(f32, f32)> = None;
        for sample in &self.history.0 {
            if last.at.saturating_duration_since(sample.at).as_secs_f32() > self.seconds as f32
                || sample.memory_total == 0
            {
                continue;
            }
            let value = percent(sample.memory_used, sample.memory_total);
            bounds = Some(bounds.map_or((value, value), |(min, max)| {
                (min.min(value), max.max(value))
            }));
        }
        let Some((min, max)) = bounds else {
            return (0.0, 100.0);
        };
        // 10% padding on either side, with at least a two-percentage-point span.
        let span = ((max - min) * 1.2).clamp(2.0, 100.0);
        let low = ((min + max - span) / 2.0).clamp(0.0, 100.0 - span);
        (low, low + span)
    }

    fn value(&self, sample: &Sample, secondary: bool) -> Option<f32> {
        match self.metric {
            Metric::Cpu => sample.cpu,
            Metric::Memory => {
                (sample.memory_total > 0).then(|| percent(sample.memory_used, sample.memory_total))
            }
            Metric::Swap => {
                (sample.swap_total > 0).then(|| percent(sample.swap_used, sample.swap_total))
            }
            Metric::Network if secondary => sample.upload,
            Metric::Network => sample.download,
        }
    }
}

impl<Message> canvas::Program<Message> for Chart<'_> {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;
        if w < 2.0 || h < 2.0 {
            return vec![frame.into_geometry()];
        }
        let top = 3.0;
        let bottom = h - 3.0;
        for fraction in [0.0, 0.5, 1.0] {
            let y = top + (bottom - top) * fraction;
            frame.stroke(
                &Path::line(Point::new(0.0, y), Point::new(w, y)),
                Stroke::default().with_color(self.grid).with_width(1.0),
            );
        }
        let Some(last) = self.history.0.back() else {
            return vec![frame.into_geometry()];
        };
        let (floor, ceiling) = self.range();
        for secondary in [false, true] {
            if secondary && !matches!(self.metric, Metric::Network) {
                break;
            }
            let color = if secondary {
                self.secondary
            } else {
                self.color
            };
            let mut segment = Vec::new();
            let mut previous = None;
            let mut paint = |points: &mut Vec<Point>| {
                if points.is_empty() {
                    return;
                }
                let line = Path::new(|builder| {
                    builder.move_to(points[0]);
                    for point in points.iter().skip(1) {
                        builder.line_to(*point);
                    }
                });
                let area = Path::new(|builder| {
                    builder.move_to(Point::new(points[0].x, bottom));
                    for point in points.iter() {
                        builder.line_to(*point);
                    }
                    builder.line_to(Point::new(points.last().unwrap().x, bottom));
                    builder.close();
                });
                frame.fill(&area, Color { a: 0.09, ..color });
                frame.stroke(&line, Stroke::default().with_color(color).with_width(1.5));
                let endpoint = *points.last().unwrap();
                frame.fill(&Path::circle(endpoint, 2.0), color);
                points.clear();
            };
            for sample in &self.history.0 {
                let age = last.at.saturating_duration_since(sample.at).as_secs_f32();
                if age > self.seconds as f32 {
                    continue;
                }
                if previous.is_some_and(|previous| is_gap(previous, sample)) {
                    paint(&mut segment);
                }
                previous = Some(sample);
                if let Some(value) = self.value(sample, secondary) {
                    segment.push(Point::new(
                        2.0 + (w - 4.0) * (1.0 - age / self.seconds as f32),
                        bottom
                            - (bottom - top)
                                * ((value - floor) / (ceiling - floor)).clamp(0.0, 1.0),
                    ));
                } else {
                    paint(&mut segment);
                }
            }
            paint(&mut segment);
        }
        vec![frame.into_geometry()]
    }
}

fn is_gap(previous: &Sample, sample: &Sample) -> bool {
    // A cadence change must not turn an intentional slow interval into a gap.
    let interval = previous.interval.max(sample.interval);
    sample.at.saturating_duration_since(previous.at) > interval.mul_f32(2.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn sample(at: Instant, download: f32) -> Sample {
        Sample {
            at,
            download: Some(download),
            ..Sample::fixture()
        }
    }

    fn chart(history: &History, metric: Metric) -> Chart<'_> {
        Chart {
            history,
            metric,
            seconds: 60,
            color: Color::WHITE,
            secondary: Color::WHITE,
            grid: Color::BLACK,
        }
    }

    #[test]
    fn memory_range_uses_visible_samples_with_padding() {
        let start = Instant::now();
        let mut history = History::default();
        let mut old = sample(start, 0.0);
        old.memory_used = 5;
        history.push(old);
        for (seconds, usage) in [(61, 50), (62, 60)] {
            let mut next = sample(start + Duration::from_secs(seconds), 0.0);
            next.memory_used = usage;
            history.push(next);
        }
        assert_eq!(chart(&history, Metric::Memory).range(), (49.0, 61.0));
        assert_eq!(chart(&history, Metric::Cpu).range(), (0.0, 100.0));
        assert_eq!(chart(&history, Metric::Swap).range(), (0.0, 100.0));
    }

    #[test]
    fn memory_range_has_minimum_span_and_stays_within_percent_bounds() {
        for (usage, expected) in [(0, (0.0, 2.0)), (50, (49.0, 51.0)), (100, (98.0, 100.0))] {
            let mut history = History::default();
            let mut next = Sample::fixture();
            next.memory_used = usage;
            history.push(next);
            assert_eq!(chart(&history, Metric::Memory).range(), expected);
        }
        let mut history = History::default();
        let mut low = Sample::fixture();
        low.memory_used = 0;
        history.push(low.clone());
        let mut high = low;
        high.memory_used = 100;
        history.push(high);
        assert_eq!(chart(&history, Metric::Memory).range(), (0.0, 100.0));
    }

    #[test]
    fn missing_memory_data_has_a_safe_default_range() {
        let mut history = History::default();
        assert_eq!(chart(&history, Metric::Memory).range(), (0.0, 100.0));
        let mut missing = Sample::fixture();
        missing.memory_total = 0;
        assert_eq!(chart(&history, Metric::Memory).value(&missing, false), None);
        history.push(missing);
        assert_eq!(chart(&history, Metric::Memory).range(), (0.0, 100.0));
    }

    #[test]
    fn network_scale_uses_only_visible_history() {
        let start = Instant::now();
        let mut history = History::default();
        history.push(sample(start, 1_000_000.0));
        history.push(sample(start + Duration::from_secs(61), 2048.0));
        assert_eq!(chart(&history, Metric::Network).ceiling(), 2048.0 * 1.1);
        assert_eq!(chart(&history, Metric::Cpu).ceiling(), 100.0);
    }

    #[test]
    fn slow_updates_are_connected_but_missed_samples_leave_gaps() {
        let start = Instant::now();
        for seconds in [1, 3, 10, 60] {
            let mut previous = sample(start, 0.0);
            previous.interval = Duration::from_secs(seconds);
            let mut next = sample(start + Duration::from_secs(seconds), 0.0);
            next.interval = previous.interval;
            assert!(!is_gap(&previous, &next));
            next.at = start + Duration::from_secs(seconds * 3);
            assert!(is_gap(&previous, &next));
        }
    }

    #[test]
    fn cadence_changes_do_not_create_false_gaps() {
        let start = Instant::now();
        for (old, new) in [(60, 1), (1, 60), (10, 3), (3, 10)] {
            let mut previous = sample(start, 0.0);
            previous.interval = Duration::from_secs(old);
            // The old timer may have nearly elapsed before it was replaced.
            let mut next = sample(start + Duration::from_secs(old + new), 0.0);
            next.interval = Duration::from_secs(new);
            assert!(!is_gap(&previous, &next));
            let mut following = sample(next.at + Duration::from_secs(new * 3), 0.0);
            following.interval = next.interval;
            assert!(is_gap(&next, &following));
        }
    }

    #[test]
    fn absent_swap_is_not_plotted_as_zero_usage() {
        let sample = sample(Instant::now(), 0.0);
        let history = History::default();
        assert_eq!(chart(&history, Metric::Swap).value(&sample, false), None);
        assert_eq!(
            chart(&history, Metric::Memory).value(&sample, false),
            Some(50.0)
        );
    }
}
