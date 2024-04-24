use std::{f32::consts::PI, ops::RangeInclusive};

use nih_plug_egui::egui::{emath::Numeric, *};

pub struct Knob<'a> {
    id: Id,
    radius: f32,
    range: RangeInclusive<f64>,
    get_set: Box<dyn 'a + FnMut(Option<f64>) -> f64>,
    default: Option<f64>,
}
impl<'a> Knob<'a> {
    pub fn new<Num: emath::Numeric>(
        id: &str,
        value: &'a mut Num,
        range: RangeInclusive<Num>,
        default: Option<Num>,
    ) -> Self {
        let range = range.start().to_f64()..=range.end().to_f64();
        Self {
            id: Id::new("knob_".to_owned() + id),
            radius: 16.0,
            range,
            get_set: Box::new(|ov| {
                if let Some(v) = ov {
                    *value = Num::from_f64(v);
                }
                value.to_f64()
            }),
            default: default.map(|v| v.to_f64()),
        }
    }
}
impl<'a> Widget for Knob<'a> {
    fn ui(mut self, ui: &mut Ui) -> Response {
        ui.vertical_centered(|ui| {
            const PADDING: f32 = 4.0;
            let (mut resp, painter) = ui.allocate_painter(
                vec2((self.radius + PADDING) * 2.0, (self.radius + PADDING) * 2.0),
                Sense {
                    click: false,
                    drag: true,
                    focusable: false,
                },
            );
            let rect = resp.rect;

            // Main knob
            let knob_center = rect.left_top() + vec2(self.radius + PADDING, self.radius + PADDING);
            painter.circle(
                knob_center,
                self.radius,
                if resp.hovered() {
                    ui.style().visuals.widgets.hovered.bg_fill
                } else {
                    ui.style().visuals.widgets.active.bg_fill
                },
                Stroke::default(),
            );

            // Cut edge (Arc/Bezier/manual segments would probably be better but this is fine)
            const EDGE_WIDTH: f32 = 2.0;
            painter
                .with_clip_rect(Rect {
                    min: rect.min,
                    max: pos2(rect.max.x, rect.min.y + self.radius * 1.75 + PADDING + EDGE_WIDTH / 2.0),
                })
                .circle_stroke(
                    knob_center,
                    self.radius,
                    Stroke::new(EDGE_WIDTH, ui.style().visuals.widgets.active.bg_stroke.color),
                );

            // Edge notches
            let max_angle = (0.75f32).tan();
            let min_angle = PI - max_angle;
            let max_value_vec = vec2(max_angle.cos(), max_angle.sin());
            painter.line_segment(
                [
                    knob_center + max_value_vec * self.radius,
                    knob_center + max_value_vec * self.radius * 1.1,
                ],
                Stroke::new(EDGE_WIDTH, ui.style().visuals.widgets.active.fg_stroke.color),
            );
            let min_value_vec = vec2(min_angle.cos(), min_angle.sin());
            painter.line_segment(
                [
                    knob_center + min_value_vec * self.radius,
                    knob_center + min_value_vec * self.radius * 1.1,
                ],
                Stroke::new(EDGE_WIDTH, ui.style().visuals.widgets.active.fg_stroke.color),
            );

            // Value angle
            let mut value = (self.get_set)(None);
            // TODO: handle 0 range
            let range_sz = self.range.end() - self.range.start();
            let value_f = (value - self.range.start()) / range_sz;
            let value_angle = min_angle + (value_f as f32) * (PI * 2.0 - (min_angle - max_angle).abs());
            let value_angle_vec = vec2(value_angle.cos(), value_angle.sin());
            painter.line_segment(
                [
                    knob_center + value_angle_vec * (self.radius / 2.0),
                    knob_center + value_angle_vec * self.radius,
                ],
                Stroke::new(EDGE_WIDTH, ui.style().visuals.widgets.active.fg_stroke.color),
            );

            // Change value
            let delta_extra_id = self.id.with("delta_extra");
            let delta_extra = ui.ctx().data_mut(|d| d.get_persisted::<f64>(delta_extra_id));
            let reset_clicked = resp.hovered() && ui.input(|i| i.pointer.middle_down());
            if reset_clicked {
                value = (self.get_set)(self.default);
                ui.ctx().data_mut(|d| d.insert_persisted::<f64>(delta_extra_id, 0.0));
                resp.mark_changed();
            } else {
                let shifted = ui.input(|i| i.modifiers.shift);
                let scroll_delta = if resp.hovered() {
                    ui.input(|i| i.raw_scroll_delta.y)
                } else {
                    0.0
                };
                let drag_div = if shifted { 1500.0 } else { 150.0 };
                let drag_delta = -(resp.drag_delta().y as f64) * range_sz / drag_div;
                let change_delta = drag_delta
                    + if scroll_delta < 0.0 {
                        -1.0
                    } else if scroll_delta > 0.0 {
                        1.0
                    } else {
                        0.0
                    };
                if change_delta != 0.0 {
                    let new_value = (value + change_delta + delta_extra.unwrap_or(0.0))
                        .min(self.range.end().to_f64())
                        .max(self.range.start().to_f64());
                    value = (self.get_set)(Some(new_value));
                    ui.ctx()
                        .data_mut(|d| d.insert_persisted::<f64>(delta_extra_id, new_value - value));
                    resp.mark_changed();
                }
            }

            // Value label
            ui.label(((value * 1000.0).round() / 1000.0).to_string());
            resp
        })
        .inner
    }
}
