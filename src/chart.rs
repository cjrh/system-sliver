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

    fn value(&self, sample: &Sample, secondary: bool) -> Option<f32> {
        match self.metric {
            Metric::Cpu => sample.cpu,
            Metric::Memory => Some(percent(sample.memory_used, sample.memory_total)),
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
        let ceiling = self.ceiling();
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
                if previous.is_some_and(|at| is_gap(at, sample)) {
                    paint(&mut segment);
                }
                previous = Some(sample.at);
                if let Some(value) = self.value(sample, secondary) {
                    segment.push(Point::new(
                        2.0 + (w - 4.0) * (1.0 - age / self.seconds as f32),
                        bottom - (bottom - top) * (value / ceiling).clamp(0.0, 1.0),
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

fn is_gap(previous: std::time::Instant, sample: &Sample) -> bool {
    sample.at.duration_since(previous) > sample.interval.mul_f32(2.5)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn sample(at: Instant, download: f32) -> Sample {
        Sample {
            at,
            interval: Duration::from_secs(1),
            cpu: Some(25.0),
            cores: 4,
            memory_used: 50,
            memory_total: 100,
            swap_used: 0,
            swap_total: 0,
            download: Some(download),
            upload: Some(0.0),
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
            let mut next = sample(start + Duration::from_secs(seconds), 0.0);
            next.interval = Duration::from_secs(seconds);
            assert!(!is_gap(start, &next));
            next.at = start + Duration::from_secs(seconds * 3);
            assert!(is_gap(start, &next));
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
