use crate::{config::Config, telemetry::Telemetry, calibration::CalibrationStore};
use eframe::egui;
use std::sync::{Arc,RwLock,OnceLock};
use std::sync::Mutex;

static SNAPSHOT: OnceLock<Mutex<Option<Telemetry>>> = OnceLock::new();
static CALIBRATION: OnceLock<Mutex<CalibrationStore>> = OnceLock::new();
pub fn publish_snapshot(t:Telemetry) { *SNAPSHOT.get_or_init(||Mutex::new(None)).lock().unwrap()=Some(t); }
pub fn publish_calibration(c:CalibrationStore) { *CALIBRATION.get_or_init(||Mutex::new(CalibrationStore::default())).lock().unwrap()=c; }
fn draw_chart(ui:&mut egui::Ui, title:&str, values:&[f32], color:egui::Color32) {
    ui.label(title); let (rect,_) = ui.allocate_exact_size(egui::vec2(ui.available_width(),130.0),egui::Sense::hover());
    let painter=ui.painter_at(rect); painter.rect_filled(rect,5.0,egui::Color32::from_rgb(12,18,27));
    for i in 1..5 { let y=rect.top()+rect.height()*i as f32/5.0; painter.line_segment([egui::pos2(rect.left(),y),egui::pos2(rect.right(),y)],egui::Stroke::new(1.0_f32,egui::Color32::from_gray(45))); }
    if values.len()<2 { painter.text(rect.center(),egui::Align2::CENTER_CENTER,"Сбор данных…",egui::FontId::proportional(14.0),egui::Color32::GRAY); return; }
    let min=values.iter().copied().fold(f32::INFINITY,f32::min); let max=values.iter().copied().fold(f32::NEG_INFINITY,f32::max); let span=(max-min).max(0.001);
    let pts:Vec<egui::Pos2>=values.iter().enumerate().map(|(i,v)|egui::pos2(rect.left()+rect.width()*i as f32/(values.len()-1) as f32,rect.bottom()-6.0-((*v-min)/span)*(rect.height()-12.0))).collect();
    painter.add(egui::Shape::line(pts,egui::Stroke::new(2.0_f32,color)));
}

pub struct TcuApp { cfg:Arc<RwLock<Config>>, status:String, ui_scale:f32 }
impl TcuApp {
    pub fn new(cfg:Arc<RwLock<Config>>) -> Self { Self{cfg,status:"Ожидание телеметрии".into(),ui_scale:1.0} }
}
impl eframe::App for TcuApp {
    fn update(&mut self, ctx:&egui::Context, _frame:&mut eframe::Frame) {
        ctx.set_pixels_per_point(self.ui_scale);
        let td=SNAPSHOT.get_or_init(||Mutex::new(None)).lock().unwrap().clone();
        let cal=CALIBRATION.get_or_init(||Mutex::new(CalibrationStore::default())).lock().unwrap().clone();
        egui::TopBottomPanel::top("topbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("FH6 Virtual TCU — Rust");
                ui.separator();
                ui.label("Локальная панель управления");
                ui.separator();
                let connected = td.as_ref().map(|t| t.is_race_on != 0).unwrap_or(false);
                let led_color = if connected { egui::Color32::from_rgb(20, 220, 120) } else { egui::Color32::from_rgb(245, 70, 88) };
                let (led_rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                let pulse = if connected { 0.8 + ((ctx.input(|i| i.time) * 3.2).sin() as f32 + 1.0) * 0.9 } else { 5.0 };
                ui.painter().circle_filled(led_rect.center(), pulse, led_color);
                ui.label(if connected { "ИГРА ПОДКЛЮЧЕНА" } else { "НЕТ СВЯЗИ С FH6" });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::Slider::new(&mut self.ui_scale,0.80..=1.35).text("Масштаб").show_value(true));
                    ui.hyperlink_to("Открыть браузерную панель", "http://127.0.0.1:8765");
                });
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false,false]).show(ui, |ui| {
                if let Some(t)=td {
                    self.status="Телеметрия получена".into();
                    ui.horizontal_wrapped(|ui| {
                        ui.group(|ui| { ui.label("СКОРОСТЬ"); ui.heading(format!("{:.1} км/ч",t.speed_kmh())); });
                        ui.group(|ui| { ui.label("ОБОРОТЫ"); ui.heading(format!("{:.0} rpm",t.current_rpm)); ui.label(format!("лимит {:.0}",t.engine_max_rpm)); });
                        ui.group(|ui| { ui.label("ПЕРЕДАЧА"); ui.heading(format!("{}",t.gear)); });
                        ui.group(|ui| { ui.label("ГАЗ / ТОРМОЗ"); ui.heading(format!("{:.0}% / {:.0}%",t.throttle()*100.0,t.brake()*100.0)); });
                        ui.group(|ui| { ui.label("АВТОМОБИЛЬ"); ui.heading(format!("ID {} · {} · PI {}",t.car_ordinal,t.car_class,t.pi)); });
                    });
                } else { self.status = "Нет связи с FH6 — ожидаю телеметрию".into(); ui.label("Ожидание телеметрии FH6…"); }
                ui.label(&self.status);
                ui.separator();
                let available=ui.available_width();
                let gap=ui.spacing().item_spacing.x;
                let left_width=(available*0.62).max(300.0);
                let right_width=(available-left_width-gap).max(260.0);
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(egui::vec2(left_width,0.0),egui::Layout::top_down(egui::Align::Min),|ui| {
                        ui.group(|ui| {
                            ui.heading("АВТОКАЛИБРОВКА · текущая машина");
                            if let Some(p)=cal.active() {
                                ui.label(format!("Автомобиль ID {} · класс {} · PI {} · {} пакетов",p.car_ordinal,p.car_class,p.pi,p.samples));
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(format!("Повышение: {:.0}% RPM · {} наблюдений",p.upshift_rpm_pct*100.0,p.upshift_observations));
                                    ui.label(format!("Понижение: {:.0}% RPM · {} наблюдений",p.downshift_rpm_pct*100.0,p.downshift_observations));
                                });
                                ui.label("Изученные передаточные отношения:");
                                ui.horizontal_wrapped(|ui| { for g in 1..=10 { if p.gear_samples[g]>0 { ui.label(format!("{}: {:.4} ({} шт.)",g,p.gear_ratio[g],p.gear_samples[g])); } } });
                                if ui.button("Сбросить калибровку этой машины").clicked() { let mut store=CALIBRATION.get_or_init(||Mutex::new(CalibrationStore::default())).lock().unwrap(); store.reset_active(); }
                                ui.columns(2,|cols| {
                                    draw_chart(&mut cols[0],"Обороты · % redline",&cal.history.iter().filter(|x|x.car_ordinal==cal.active_car).map(|x|x.rpm_pct*100.0).collect::<Vec<_>>(),egui::Color32::from_rgb(57,181,255));
                                    draw_chart(&mut cols[1],"Мощность · кВт",&p.power_curve.iter().map(|x|x.power_kw).collect::<Vec<_>>(),egui::Color32::from_rgb(66,211,146));
                                });
                            } else { ui.label("Выбери машину в FH6 и проедь несколько разгонов — профиль появится автоматически."); }
                        });
                    });
                    ui.allocate_ui_with_layout(egui::vec2(right_width,0.0),egui::Layout::top_down(egui::Align::Min),|ui| {
                        ui.group(|ui| {
                            ui.heading("УПРАВЛЕНИЕ ПЕРЕКЛЮЧЕНИЯМИ");
                            let mut c=self.cfg.write().unwrap();
                            ui.checkbox(&mut c.automatic,"Автоматическое переключение");
                            ui.horizontal(|ui| { ui.label("Режим"); egui::ComboBox::from_id_source("mode").selected_text(&c.mode).show_ui(ui,|ui|{ for m in ["Comfort","Race","Drift","Drag","Off-road"] { ui.selectable_value(&mut c.mode,m.to_string(),m); } }); });
                            ui.add(egui::Slider::new(&mut c.upshift_rpm_pct,0.5..=0.99).text("Повышение · redline"));
                            ui.add(egui::Slider::new(&mut c.downshift_rpm_pct,0.15..=0.75).text("Понижение · redline"));
                            ui.add(egui::Slider::new(&mut c.shift_delay_ms,100..=1200).text("Задержка между переключениями · мс"));
                            ui.label(match c.mode.as_str() { "Drag" => "Drag: удержание передачи до высоких RPM, плавная защита от лишних переключений.", "Drift" => "Drift: быстрый отклик, более раннее понижение и меньше блокировок при контролируемом заносе.", _ => "Задержка задаёт минимальный интервал между автоматическими командами." });
                            ui.horizontal(|ui| { ui.label("Вверх"); ui.text_edit_singleline(&mut c.key_up); ui.label("Вниз"); ui.text_edit_singleline(&mut c.key_down); });
                            ui.horizontal(|ui| { ui.label("UDP"); ui.add(egui::DragValue::new(&mut c.udp_port).clamp_range(1..=65535)); ui.label("Web"); ui.add(egui::DragValue::new(&mut c.web_port).clamp_range(1..=65535)); });
                            ui.separator();
                            ui.heading("FEATURES · алгоритмы");
                            ui.columns(2,|cols| {
                                cols[0].checkbox(&mut c.features.cornering_lock,"Cornering Lock");
                                cols[0].checkbox(&mut c.features.brake_curve,"Brake Curve");
                                cols[0].checkbox(&mut c.features.shift_advisor_manual,"Shift Advisor");
                                cols[0].checkbox(&mut c.features.power_curve_learning,"Power Curve Learning");
                                cols[0].checkbox(&mut c.features.airtime_lock,"Airtime Lock");
                                cols[0].checkbox(&mut c.features.drive_style_adaptive,"Drive Style Adaptive");
                                cols[1].checkbox(&mut c.features.launch_control,"Launch Control");
                                cols[1].checkbox(&mut c.features.drivetrain_aware,"Drivetrain Aware");
                                cols[1].checkbox(&mut c.features.engine_brake,"Engine Brake");
                                cols[1].checkbox(&mut c.features.turbo_lag_compensation,"Turbo Lag Compensation");
                                cols[1].checkbox(&mut c.features.transient_lock,"Transient Lock");
                            });
                            if ui.button("Сохранить настройки").clicked() {
                                if let Err(e)=c.save() { self.status=format!("Ошибка сохранения: {e}"); }
                                else { self.status="Настройки сохранены".into(); }
                            }
                        });
                    });
                });
            });
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
}
