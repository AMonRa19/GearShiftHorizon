use crate::{config::Config, telemetry::Telemetry};
use eframe::egui;
use std::sync::{Arc,RwLock,OnceLock};
use std::sync::Mutex;

static SNAPSHOT: OnceLock<Mutex<Option<Telemetry>>> = OnceLock::new();
pub fn publish_snapshot(t:Telemetry) { *SNAPSHOT.get_or_init(||Mutex::new(None)).lock().unwrap()=Some(t); }

pub struct TcuApp { cfg:Arc<RwLock<Config>>, status:String }
impl TcuApp {
    pub fn new(cfg:Arc<RwLock<Config>>) -> Self { Self{cfg,status:"Ожидание телеметрии".into()} }
}
impl eframe::App for TcuApp {
    fn update(&mut self, ctx:&egui::Context, _frame:&mut eframe::Frame) {
        let td=SNAPSHOT.get_or_init(||Mutex::new(None)).lock().unwrap().clone();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("FH6 Virtual TCU — Rust");
            ui.label("Начальная Rust-реализация ядра; функциональность переносится поэтапно.");
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
            let mut c=self.cfg.write().unwrap();
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
            ui.horizontal(|ui| { ui.label("UDP порт"); ui.add(egui::DragValue::new(&mut c.udp_port).clamp_range(1..=65535)); });
            if ui.button("Сохранить настройки").clicked() {
                if let Err(e)=c.save() { self.status=format!("Ошибка сохранения: {e}"); }
                else { self.status="Настройки сохранены".into(); }
            }
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
}
