use crate::{config::Config, telemetry::Telemetry, calibration::CalibrationStore};
use eframe::egui;
use std::sync::{Arc,RwLock,OnceLock};
use std::sync::Mutex;

static SNAPSHOT: OnceLock<Mutex<Option<Telemetry>>> = OnceLock::new();
static CALIBRATION: OnceLock<Mutex<CalibrationStore>> = OnceLock::new();
pub fn publish_snapshot(t:Telemetry) { *SNAPSHOT.get_or_init(||Mutex::new(None)).lock().unwrap()=Some(t); }
pub fn publish_calibration(c:CalibrationStore) { *CALIBRATION.get_or_init(||Mutex::new(CalibrationStore::default())).lock().unwrap()=c; }
fn thirty_two() -> f32 { 32.0 }
fn forty_eight() -> f32 { 48.0 }

fn draw_chart(ui:&mut egui::Ui, title:&str, values:&[f32], color:egui::Color32) {
    ui.label(title); let (rect,_) = ui.allocate_exact_size(egui::vec2(ui.available_width(),130.0),egui::Sense::hover());
    let painter=ui.painter_at(rect); painter.rect_filled(rect,5.0,egui::Color32::from_rgb(12,18,27));
    for i in 1..5 { let y=rect.top()+rect.height()*i as f32/5.0; painter.line_segment([egui::pos2(rect.left(),y),egui::pos2(rect.right(),y)],egui::Stroke::new(1.0_f32,egui::Color32::from_gray(45))); }
    if values.len()<2 { painter.text(rect.center(),egui::Align2::CENTER_CENTER,"Сбор данных…",egui::FontId::proportional(14.0),egui::Color32::GRAY); return; }
    let min=values.iter().copied().fold(f32::INFINITY,f32::min); let max=values.iter().copied().fold(f32::NEG_INFINITY,f32::max); let span=(max-min).max(0.001);
    let pts:Vec<egui::Pos2>=values.iter().enumerate().map(|(i,v)|egui::pos2(rect.left()+rect.width()*i as f32/(values.len()-1) as f32,rect.bottom()-6.0-((*v-min)/span)*(rect.height()-12.0))).collect();
    painter.add(egui::Shape::line(pts,egui::Stroke::new(2.0_f32,color)));
}

pub struct TcuApp { cfg:Arc<RwLock<Config>>, status:String, ui_scale:f32, hud_enabled:bool }
impl TcuApp {
    pub fn new(cfg:Arc<RwLock<Config>>) -> Self { Self{cfg,status:"Ожидание телеметрии".into(),ui_scale:1.0,hud_enabled:false} }
}
impl eframe::App for TcuApp {
    fn update(&mut self, ctx:&egui::Context, _frame:&mut eframe::Frame) {
        ctx.set_pixels_per_point(self.ui_scale);
        let td=SNAPSHOT.get_or_init(||Mutex::new(None)).lock().unwrap().clone();
        let cal=CALIBRATION.get_or_init(||Mutex::new(CalibrationStore::default())).lock().unwrap().clone();
        egui::TopBottomPanel::top("topbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("GearShiftHorizon · Rust Core");
                ui.separator();
                ui.checkbox(&mut self.hud_enabled, "HUD поверх игры");
                ui.label("Локальная панель управления");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::Slider::new(&mut self.ui_scale,0.80..=1.35).text("Масштаб").show_value(true));
                    ui.hyperlink_to("Открыть браузерную панель", "http://127.0.0.1:8765");
                });
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false,false]).show(ui, |ui| {
                if let Some(t)=td.as_ref() {
                    self.status="Телеметрия получена".into();
                    ui.horizontal_wrapped(|ui| {
                        ui.group(|ui| { ui.label("СКОРОСТЬ"); ui.heading(format!("{:.1} км/ч",t.speed_kmh())); });
                        ui.group(|ui| { ui.label("ОБОРОТЫ"); ui.heading(format!("{:.0} rpm",t.current_rpm)); ui.label(format!("лимит {:.0}",t.engine_max_rpm)); });
                        ui.group(|ui| { ui.label("ПЕРЕДАЧА"); ui.heading(format!("{}",t.gear)); });
                        ui.group(|ui| { ui.label("ГАЗ / ТОРМОЗ"); ui.heading(format!("{:.0}% / {:.0}%",t.throttle()*100.0,t.brake()*100.0)); });
                        ui.group(|ui| { ui.label("АВТОМОБИЛЬ"); ui.heading(format!("ID {} · {} · PI {}",t.car_ordinal,t.car_class,t.pi)); });
                    });
                } else { ui.label("Ожидание телеметрии FH6…"); }
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
                                ui.label(format!("Достоверность обучения: {:.0}% · стиль езды: {:.0}% агрессивности",p.learning_confidence*100.0,p.drive_style_aggression*100.0));
                                ui.label(format!("Точки мощности: {} · передачи с обучением ускорения: {}",p.power_curve.iter().filter(|x|x.samples>=12).count(),p.gear_accel_samples.iter().filter(|n|**n>=20).count()));
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(format!("Повышение: {:.0}% RPM · {} наблюдений",p.upshift_rpm_pct*100.0,p.upshift_observations));
                                    ui.label(format!("Понижение: {:.0}% RPM · {} наблюдений",p.downshift_rpm_pct*100.0,p.downshift_observations));
                                });
                                ui.label("Изученные передаточные отношения:");
                                ui.horizontal_wrapped(|ui| { for g in 1..=10 { if p.gear_samples[g]>=30 { ui.label(format!("{}: {:.4} ({} шт.)",g,p.gear_ratio[g],p.gear_samples[g])); } } });
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
                            ui.label("Интервал переключения: 350 мс · оригинальный алгоритм");
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
        if self.hud_enabled {
            let mut close_hud = false;
            let hud_id = egui::ViewportId::from_hash_of("gearshift_horizon_hud");
            ctx.show_viewport_immediate(
                hud_id,
                egui::ViewportBuilder::default()
                    .with_title("GearShiftHorizon HUD")
                    .with_inner_size([340.0, 176.0])
                    .with_min_inner_size([300.0, 150.0])
                    .with_decorations(false)
                    .with_always_on_top(),
                |hud_ctx, _class| {
                    if hud_ctx.input(|i| i.viewport().close_requested()) {
                        close_hud = true;
                    }
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none().fill(egui::Color32::from_rgba_unmultiplied(12, 18, 28, 238)))
                        .show(hud_ctx, |ui| {
                            ui.horizontal(|ui| {
                                ui.strong("GEARSHIFT HUD");
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("×").clicked() { close_hud = true; }
                                });
                            });
                            ui.separator();
                            if let Some(t) = td.as_ref() {
                                let pct = (t.rpm_pct().clamp(0.0, 1.0)) as f64;
                                let color = if pct >= 0.94 { egui::Color32::from_rgb(255, 82, 92) }
                                    else if pct >= 0.82 { egui::Color32::from_rgb(255, 190, 82) }
                                    else { egui::Color32::from_rgb(54, 190, 245) };
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label("ОБОРОТЫ");
                                        ui.heading(egui::RichText::new(format!("{:.0}", t.current_rpm)).size( thirty_two()).strong());
                                        ui.label(format!("из {:.0} RPM", t.engine_max_rpm));
                                    });
                                    ui.separator();
                                    ui.vertical(|ui| {
                                        ui.label("ПЕРЕДАЧА");
                                        ui.heading(egui::RichText::new(format!("{}", t.gear)).size( forty_eight()).strong());
                                        ui.label(format!("{:.0} км/ч", t.speed_kmh()));
                                    });
                                });
                                ui.add(egui::ProgressBar::new(pct as f32).fill(color).text(format!("{:.0}% RPM", pct * 100.0)));
                                ui.label(format!("Газ {:.0}%   Тормоз {:.0}%   Ручник {:.0}%",
                                    t.throttle()*100.0, t.brake()*100.0, t.handbrake()*100.0));
                            } else {
                                ui.centered_and_justified(|ui| { ui.label("Ожидание телеметрии FH6…"); });
                            }
                        });
                    hud_ctx.request_repaint_after(std::time::Duration::from_millis(33));
                },
            );
            if close_hud { self.hud_enabled = false; }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
}
