use eframe::{egui, App as EApp, Frame};
use html_escape::decode_html_entities;
use regex::Regex;
use reqwest::blocking::Client;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::Duration;
use url::Url;

#[derive(Default)]
struct RokuRemoteApp {
    devices: Vec<String>,
    selected_device: Option<String>,
    apps: Vec<AppEntry>,
    selected_app: Option<String>,
    last_msg: String,
    text_input: String,
}

#[derive(Debug, Clone)]
struct AppEntry {
    id: String,
    name: String,
}

// establish a list of roku commands
const ROKU_COMMANDS: &[&[&str]] = &[
    &["Power", "Poweron", "Poweroff"],
    &["Home", "Info", "Back"],
    &[" ", " ", " "],
    &[" ", "Up", " "],
    &["Left", "Select", "Right"],
    &[" ", "Down", " "],
    &[" ", " ", "Play"],
    &["VolumeUp", "VolumeDown", "VolumeMute"],
    &["Channel_up", "Channel_down", "Search"],
    &["Enter", "Backspace", "Find_remote"],
    &["Replay", "Reverse", "Forward"],
];
// app
fn main() -> Result<(), eframe::Error> {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "🦀 Roku Remote",
        native_options,
        Box::new(|_cc| Box::new(RokuRemoteApp::default())),
    )
}

impl EApp for RokuRemoteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Roku Remote");

            if ui.button("Discover Roku Devices").clicked() {
                self.devices = discover_roku_devices();
                self.devices.sort();
                self.devices.dedup();
                self.last_msg = format!("Found {} device(s)", self.devices.len());
            }

            if !self.devices.is_empty() {
                ui.separator();
                ui.label("Select a Roku Device:");

                egui::ComboBox::from_label("Devices")
                    .selected_text(self.selected_device.clone().unwrap_or_else(|| "None".into()))
                    .show_ui(ui, |ui| {
                        for device in &self.devices {
                            if ui
                                .selectable_label(Some(device) == self.selected_device.as_ref(), device)
                                .clicked()
                            {
                                self.selected_device = Some(device.clone());
                                self.apps = get_apps(device);
                                self.last_msg = format!("Fetched {} apps", self.apps.len());
                            }
                        }
                    });

                ui.separator();
                ui.label("Commands:");

                if let Some(ip) = &self.selected_device {
                    egui::Grid::new("commands_grid")
                        .num_columns(3)
                        .min_col_width(100.0) 
                        .spacing([10.0, 10.0]) 
                        .show(ui, |ui| {
                            for row in ROKU_COMMANDS {
                                for &cmd in *row {
                                    if cmd != " " {
                                        // Create a fixed-size button with centered text
                                        ui.allocate_ui(egui::vec2(60.0, 20.0), |ui| {
                                            ui.with_layout(
                                                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                                                |ui| {
                                                    if ui.button(cmd).clicked() {
                                                        send_command(ip, cmd);
                                                        self.last_msg = format!("Sent command: {}", cmd);
                                                    }
                                                },
                                            );
                                        });
                                    } else {
                                        
                                        ui.label("");
                                    }
                                }
                                ui.end_row();
                            }
                        });

                    ui.separator();
                    ui.label("Send Text Input:");
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.text_input);
                        if ui.button("Send Text").clicked() {
                            if !self.text_input.trim().is_empty() {
                                send_key(ip, &self.text_input);
                                self.last_msg = format!("Sent text: {}", self.text_input);
                                self.text_input.clear();
                            }
                        }
                    });
                } else {
                    ui.label("No Roku selected");
                }

                ui.separator();
                ui.label("Apps:");
                egui::ComboBox::from_label("Pick an App")
                    .selected_text(
                        self.selected_app
                            .as_ref()
                            .and_then(|app_id| {
                                self.apps
                                    .iter()
                                    .find(|app| app.id == *app_id)
                                    .map(|app| app.name.clone())
                            })
                            .unwrap_or_else(|| "None".into()),
                    )
                    .show_ui(ui, |ui| {
                        for app in &self.apps {
                            if ui
                                .selectable_label(Some(app.id.clone()) == self.selected_app, &app.name)
                                .clicked()
                            {
                                self.selected_app = Some(app.id.clone());
                            }
                        }
                    });

                if ui.button("Launch App").clicked() {
                    if let (Some(ip), Some(app_id)) = (&self.selected_device, &self.selected_app) {
                        launch_app(ip, app_id);
                        let app_name = self
                            .apps
                            .iter()
                            .find(|app| app.id == *app_id)
                            .map(|app| app.name.clone())
                            .unwrap_or_else(|| "Unknown App".to_string());
                        self.last_msg = format!("Launching app: {}", app_name);
                    }
                }
            }

            ui.separator();
            ui.label(format!("Status: {}", self.last_msg));
        });
    }
}
// discover roku devices on the network using SSDP(simple service discovery protocol)
fn discover_roku_devices() -> Vec<String> {
    const SSDP_ADDR: &str = "239.255.255.250";
    const SSDP_PORT: u16 = 1900;
    const ST: &str = "roku:ecp";
    const TIMEOUT_SECS: u64 = 2;
    const RETRIES: usize = 1;

    let dest = SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), SSDP_PORT);
    let msg = format!(
        "M-SEARCH * HTTP/1.1\r\n\
         HOST: {SSDP_ADDR}:{SSDP_PORT}\r\n\
         MAN: \"ssdp:discover\"\r\n\
         ST: {ST}\r\n\
         MX: 3\r\n\r\n"
    );

    let mut found = Vec::new();

    for _ in 0..RETRIES {
        let sock = UdpSocket::bind("0.0.0.0:0").expect("bind failed");
        sock.set_read_timeout(Some(Duration::from_secs(TIMEOUT_SECS))).ok();
        sock.set_multicast_loop_v4(true).ok();
        sock.set_multicast_ttl_v4(4).ok();
        sock.send_to(msg.as_bytes(), dest).ok();

        let mut buf = [0u8; 2048];
        loop {
            match sock.recv_from(&mut buf) {
                Ok((amt, _)) => {
                    let data = String::from_utf8_lossy(&buf[..amt]);
                    if let Some(line) = data
                        .lines()
                        .find(|l| l.len() >= 9 && l[..9].eq_ignore_ascii_case("location:"))
                    {
                        let location = line[9..].trim();
                        if let Ok(url) = Url::parse(location) {
                            if let (Some(host), Some(port)) = (url.host_str(), url.port()) {
                                let address = format!("{}:{}", host, port);
                                if !found.contains(&address) {
                                    found.push(address);
                                }
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }
    }

    found
}
// query available apps to create a list and launch apps directly 
fn get_apps(ip: &str) -> Vec<AppEntry> {
    let url = format!("http://{}/query/apps", ip);
    let client = Client::new();

    if let Ok(resp) = client.get(&url).send() {
        if let Ok(text) = resp.text() {
            let re = Regex::new(r#"<app[^>]*id="([^"]+)"[^>]*>(.*?)</app>"#).unwrap();
            return re
                .captures_iter(&text)
                .map(|cap| AppEntry {
                    id: cap[1].to_string(),
                    name: decode_html_entities(&cap[2]).to_string(),
                })
                .collect();
        }
    }

    vec![]
}
// form commands and send over the network using http
fn send_command(ip: &str, command: &str) {
    let url = format!("http://{}/keypress/{}", ip, command);
    let _ = Client::new().post(&url).send();
}
// launch specific apps without having to manually navigate to them
fn launch_app(ip: &str, app_id: &str) {
    let url = format!("http://{}/launch/{}", ip, app_id);
    let _ = Client::new().post(&url).send();
}
// send strings to roku device 
// the literal function only sends one character at a time  
// so for loop 
fn send_key(ip: &str, key: &str) {
    let client = Client::new();
    for c in key.chars() {
        let encoded_char = if c == ' ' {
            "%20".to_string()
        } else {
            c.to_string()
        };
        let url = format!("http://{}/keypress/Lit_{}", ip, encoded_char);
        let _ = client.post(&url).send();
    }
}