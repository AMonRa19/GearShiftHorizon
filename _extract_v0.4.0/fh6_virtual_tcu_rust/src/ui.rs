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
    for i in 1..5 { let y=rect.top()+rect.height()*i as f32/5.0; painter.line_segment([egui::pos2(rect.left(),y),egui::pos2(rect.right(),y)],egui::Stroke::new(1.0,egui::Color32::from_gray(45))); }
    if values.len()<2 { painter.text(rect.center(),egui::Align2::CENTER_CENTER,"Сбор данных…",egui::FontId::proportional(14.0),egui::Color32::GRAY); return; }
    let min=values.iter().copied().fold(f32::INFINITY,f32::min); let max=values.iter().copied().fold(f32::NEG_INFINITY,f32::max); let span=(max-min).max(0.001);
    let pts:Vec<egui::Pos2>=values.iter().enumerate().map(|(i,v)|egui::pos2(rect.left()+rect.width()*i as f32/(values.len()-1) as f32,rect.bottom()-6.0-((*v-min)/span)*(rect.height()-12.0))).collect();
    painter.add(egui::Shape::line(pts,egui::Stroke::new(2.0,color)));
}

pub struct TcuApp { cfg:Arc<RwLock<Config>>, status:String }
impl TcuApp {
    pub fn new(cfg:Arc<RwLock<Config>>) -> Self { Self{cfg,status:"Ожидание телеметрии".into()} }
}
impl eframe::App for TcuApp {
    fn update(&mut self, ctx:&egui::Context, _frame:&mut eframe::Frame) {
        let td=SNAPSHOT.get_or_init(||Mutex::new(None)).lock().unwrap().clone();
        let cal=CALIBRATION.get_or_init(||Mutex::new(CalibrationStore::default())).lock().unwrap().clone();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("FH6 Virtual TCU — Rust");
            ui.label("Rust TCU · локальная панель управления");
            ui.label("Браузерная панель: http://127.0.0.1:8765");
            ui.separator();
            if let Some(t)=td {
                self.status="Телеметрия получена".into();
                egui::Grid::new("telemetry").show(ui,|ui|{
                    ui.label("Скорость"); ui.label(format!("{:.1} км/ч",t.speed_kmh())); ui.end_row();
                    ui.label("Обороты"); ui.label(format!("{:.0} / {:.0} rpm",t.current_rpm,t.engine_max_rpm)); ui.end_row();
                    ui.label("Передача"); ui.label(format!("{}",t.gear)); ui.end_row();
                    ui.label("Газ / тормоз"); ui.label(format!("{:.0}% / {:.0}%",t.throttle()*100.0,t.brake()*100.0)); ui.end_row();
                    ui.label("Автомобиль"); ui.label(format!("ID {} · класс {} · PI {}",t.car_ordinal,t.car_class,t.pi)); ui.end_row();
                });
            }
            ui.label(&self.status);
            ui.separator();
            ui.collapsing("АВТОКАЛИБРОВКА · профиль текущей машины", |ui| {
                if let Some(p)=cal.active() {
                    ui.label(format!("Автомобиль ID {} · класс {} · PI {} · выборка {} пакетов",p.car_ordinal,p.car_class,p.pi,p.samples));
                    ui.label(format!("Повышение: {:.0}% RPM · наблюдений {}",p.upshift_rpm_pct*100.0,p.upshift_observations));
                    ui.label(format!("Понижение: {:.0}% RPM · наблюдений {}",p.downshift_rpm_pct*100.0,p.downshift_observations));
                    ui.label("Передачи с обученным отношением:");
                    ui.horizontal_wrapped(|ui| { for g in 1..=10 { if p.gear_samples[g]>0 { ui.label(format!("{}: {:.5} ({} сэмп.)",g,p.gear_ratio[g],p.gear_samples[g])); } } });
                    if ui.button("Сбросить калибровку этой машины").clicked() { let mut store=CALIBRATION.get_or_init(||Mutex::new(CalibrationStore::default())).lock().unwrap(); store.reset_active(); }
                    draw_chart(ui,"Обороты двигателя · % от redline",&cal.history.iter().filter(|x|x.car_ordinal==cal.active_car).map(|x|x.rpm_pct*100.0).collect::<Vec<_>>(),egui::Color32::from_rgb(57,181,255));
                    draw_chart(ui,"Мощность · кВт",&p.power_curve.iter().map(|x|x.power_kw).collect::<Vec<_>>(),egui::Color32::from_rgb(66,211,146));
                } else { ui.label("Выбери машину в FH6 и проедь несколько разгонов — профиль появится автоматически."); }
            });
            ui.separator();
            let mut c=self.cfg.write().unwrap();
            ui.collapsing("FEATURES · алгоритмы", |ui| {
                ui.checkbox(&mut c.features.cornering_lock, "Cornering Lock — блокировка переключений в повороте");
                ui.checkbox(&mut c.features.brake_curve, "Brake Curve — адаптация по тормозу");
                ui.checkbox(&mut c.features.shift_advisor_manual, "Shift Advisor (Manual) — подсказки ручного режима");
                ui.checkbox(&mut c.features.power_curve_learning, "Power Curve Learning — обучение кривой мощности");
                ui.checkbox(&mut c.features.airtime_lock, "Airtime Lock (jumps) — блокировка в воздухе");
                ui.checkbox(&mut c.features.drive_style_adaptive, "Drive Style (adaptive) — адаптивный стиль");
                ui.checkbox(&mut c.features.launch_control, "Launch Control — стартовый контроль");
                ui.checkbox(&mut c.features.drivetrain_aware, "Drivetrain Aware — учёт типа привода");
                ui.checkbox(&mut c.features.engine_brake, "Engine Brake (descents) — торможение двигателем");
                ui.checkbox(&mut c.features.turbo_lag_compensation, "Turbo Lag Compensation — компенсация турбоямы");
                ui.checkbox(&mut c.features.transient_lock, "Transient Lock (oversteer) — защита от резких заносов");
            });
            ui.separator();
            ui.checkbox(&mut c.automatic,"Автоматическое переключение");
            ui.horizontal(|ui|{
                ui.label("Режим");
                egui::ComboBox::from_id_source("mode").selected_text(&c.mode).show_ui(ui,|ui|{
                    for m in ["Comfort","Race","Drift","Off-road"] { ui.selectable_value(&mut c.mode,m.to_string(),m); }
                });
            });
            ui.horizontal(|ui| { ui.label("Повышение"); ui.add(egui::Slider::new(&mut c.upshift_rpm_pct,0.5..=0.99).suffix(" redline")); });
            ui.horizontal(|ui| { ui.label("Понижение"); ui.add(egui::Slider::new(&mut c.downshift_rpm_pct,0.15..=0.75).suffix(" redline")); });
            ui.horizontal(|ui| { ui.label("Клавиша вверх"); ui.text_edit_singleline(&mut c.key_up); ui.label("вниз"); ui.text_edit_singleline(&mut c.key_down); });
            ui.horizontal(|ui| { ui.label("UDP порт"); ui.add(egui::DragValue::new(&mut c.udp_port).clamp_range(1..=65535)); ui.label("Web порт"); ui.add(egui::DragValue::new(&mut c.web_port).clamp_range(1..=65535)); });
            if ui.button("Сохранить настройки").clicked() {
                if let Err(e)=c.save() { self.status=format!("Ошибка сохранения: {e}"); }
                else { self.status="Настройки сохранены".into(); }
            }
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
}
