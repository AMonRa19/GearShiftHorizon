use crate::config::Config;
use std::{io::{Read, Write}, net::{TcpListener, TcpStream}, sync::{Arc, RwLock}, thread};

const PAGE: &str = r#"<!doctype html><html lang="ru"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>FH6 Virtual TCU</title><style>
:root{color-scheme:dark}*{box-sizing:border-box}body{margin:0;background:#0b1018;color:#e8edf5;font:15px Segoe UI,Arial}header{padding:22px 5vw;background:#121c2a;border-bottom:1px solid #263449;display:flex;justify-content:space-between;align-items:center}h1{font-size:22px;margin:0}small,.muted{color:#8fa1b8}.wrap{max-width:1050px;margin:28px auto;padding:0 20px}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(290px,1fr));gap:14px}.card{background:#121c2a;border:1px solid #263449;border-radius:12px;padding:18px}.card h2{font-size:16px;margin:0 0 14px}.feature{display:flex;gap:12px;align-items:flex-start;padding:10px 0;border-bottom:1px solid #263449}.feature:last-child{border:0}.feature input{accent-color:#39b5ff;width:17px;height:17px;margin-top:2px}.feature b{display:block;font-size:14px}.feature small{display:block;margin-top:3px;line-height:1.35}label.field{display:flex;justify-content:space-between;gap:12px;margin:12px 0;align-items:center}input[type=text],input[type=number],select{background:#0b1018;color:#fff;border:1px solid #34465f;border-radius:7px;padding:8px;width:120px}button{background:#168bd0;color:white;border:0;border-radius:8px;padding:11px 18px;font-weight:600;cursor:pointer;margin-top:16px}#status{margin-left:12px;color:#8fa1b8}.topline{display:flex;gap:12px;align-items:center;flex-wrap:wrap}</style></head><body><header><div><h1>FH6 Virtual TCU <span class="muted">/ RUST</span></h1><small>Локальная панель управления</small></div><div class="muted">Backend · 127.0.0.1:8765</div></header><main class="wrap"><div class="topline"><h2>Настройки контроллера</h2><span id="status">Загрузка…</span></div><div class="grid"><section class="card"><h2>FEATURES</h2><div id="features"></div></section><section class="card"><h2>Переключение передач</h2><label class="field">Автоматический режим <input id="automatic" type="checkbox"></label><label class="field">Профиль <select id="mode"><option>Comfort</option><option>Race</option><option>Drift</option><option>Off-road</option></select></label><label class="field">Повышение, доля RPM <input id="upshift_rpm_pct" type="number" min="0.5" max="0.99" step="0.01"></label><label class="field">Понижение, доля RPM <input id="downshift_rpm_pct" type="number" min="0.15" max="0.75" step="0.01"></label><label class="field">Клавиша вверх <input id="key_up" type="text"></label><label class="field">Клавиша вниз <input id="key_down" type="text"></label><label class="field">UDP порт <input id="udp_port" type="number"></label></section></div><button onclick="save()">Сохранить настройки</button><span id="saveStatus"></span></main><script>
const defs=[['cornering_lock','Cornering Lock','Не переключает передачи при высокой боковой перегрузке.'],['brake_curve','Brake Curve','Учитывает интенсивность торможения при выборе понижения.'],['shift_advisor_manual','Shift Advisor (Manual)','Подсказывает оптимальный момент ручного переключения.'],['power_curve_learning','Power Curve Learning','Обучается рабочему диапазону мощности двигателя.'],['airtime_lock','Airtime Lock (jumps)','Блокирует переключения во время прыжка.'],['drive_style_adaptive','Drive Style (adaptive)','Подстраивает характер переключений под манеру езды.'],['launch_control','Launch Control','Помогает контролировать старт с места.'],['drivetrain_aware','Drivetrain Aware','Учитывает тип привода при выборе передачи.'],['engine_brake','Engine Brake (descents)','Учитывает спуски и торможение двигателем.'],['turbo_lag_compensation','Turbo Lag Compensation','Компенсирует задержку наддува при выборе передачи.'],['transient_lock','Transient Lock (oversteer)','Защищает от переключения при резком срыве задней оси.']];
const root=document.querySelector('#features');defs.forEach(([k,n,d])=>{let el=document.createElement('label');el.className='feature';el.innerHTML=`<input type="checkbox" id="f_${k}"><span><b>${n}</b><small>${d}</small></span>`;root.append(el)});
async function load(){try{let c=await(await fetch('/api/config')).json();for(let k of ['automatic','mode','upshift_rpm_pct','downshift_rpm_pct','key_up','key_down','udp_port'])document.getElementById(k).value=c[k],document.getElementById(k).type==='checkbox'&&(document.getElementById(k).checked=c[k]);for(let [k] of defs)document.getElementById('f_'+k).checked=!!c.features[k];document.querySelector('#status').textContent='Подключено';}catch(e){document.querySelector('#status').textContent='Нет связи с TCU'}}
async function save(){let c=await(await fetch('/api/config')).json();for(let k of ['automatic','mode','upshift_rpm_pct','downshift_rpm_pct','key_up','key_down','udp_port']){let e=document.getElementById(k);c[k]=e.type==='checkbox'?e.checked:(e.type==='number'?Number(e.value):e.value)}for(let [k] of defs)c.features[k]=document.getElementById('f_'+k).checked;let r=await fetch('/api/config',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(c)});document.querySelector('#saveStatus').textContent=r.ok?'Сохранено ✓':'Ошибка сохранения';}
load();setInterval(load,5000);
</script></body></html>"#;

fn respond(mut s: TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n", body.len());
    let _ = s.write_all(head.as_bytes()); let _ = s.write_all(body);
}
fn handle(mut stream: TcpStream, cfg: Arc<RwLock<Config>>) {
    let mut buf=vec![0u8;65536]; let n=match stream.read(&mut buf){Ok(n)=>n,Err(_)=>return};
    let req=String::from_utf8_lossy(&buf[..n]); let mut lines=req.split("\r\n"); let first=lines.next().unwrap_or("");
    let mut parts=first.split_whitespace(); let method=parts.next().unwrap_or(""); let path=parts.next().unwrap_or("/");
    if method=="GET" && path=="/" {respond(stream,"200 OK","text/html; charset=utf-8",PAGE.as_bytes());return;}
    if method=="GET" && path=="/api/config" {let data=serde_json::to_vec(&*cfg.read().unwrap()).unwrap_or_default();respond(stream,"200 OK","application/json",&data);return;}
    if method=="POST" && path=="/api/config" {
        let len=lines.clone().find_map(|l|{let (k,v)=l.split_once(':')?;if k.eq_ignore_ascii_case("content-length"){v.trim().parse::<usize>().ok()}else{None}}).unwrap_or(0);
        let body_start=req.find("\r\n\r\n").map(|i|i+4).unwrap_or(n); let mut body=buf[body_start..n].to_vec();
        while body.len()<len {let mut extra=vec![0u8;len-body.len()];match stream.read(&mut extra){Ok(0)|Err(_)=>break,Ok(m)=>body.extend_from_slice(&extra[..m])}}
        match serde_json::from_slice::<Config>(&body){Ok(c)=>{let mut g=cfg.write().unwrap();*g=c.clone();let ok=c.save().is_ok();respond(stream,if ok{"200 OK"}else{"500 Internal Server Error"},"application/json",if ok{b"{\"ok\":true}"}else{b"{\"ok\":false}"});},Err(_)=>respond(stream,"400 Bad Request","application/json",b"{\"ok\":false}"),}
        return;
    }
    respond(stream,"404 Not Found","text/plain",b"not found");
}
pub fn start(cfg: Arc<RwLock<Config>>) {
    thread::spawn(move || loop {
        let port=cfg.read().unwrap().web_port;
        match TcpListener::bind(("127.0.0.1",port)) {
            Ok(listener)=>{for stream in listener.incoming(){if let Ok(s)=stream{handle(s,Arc::clone(&cfg));}}}
            Err(e)=>{eprintln!("Web UI: cannot bind 127.0.0.1:{port}: {e}");thread::sleep(std::time::Duration::from_secs(3));}
        }
    });
}
