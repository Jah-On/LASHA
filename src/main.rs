use eframe::{egui::{self, vec2}, epi, NativeOptions, run_native};
use std::{time::Duration, future, default, rc::{self, Rc}, sync::{Arc, Mutex}, pin::Pin, task::Context};

struct App {
    asha: Arc<Mutex<ASHA::ASHA>>
}

impl epi::App for App {
    fn name(&self) -> &str {
        "LASHA"
    }

    fn update(&mut self, ctx: &egui::CtxRef, frame: &epi::Frame) {
        ctx.set_pixels_per_point(2.0);

        egui::CentralPanel::default().show(ctx, |ui| {
            // match self.asha.as_ref() {
            //     ASHA::AdapterState::NoAdapter    => {
            //         ui.heading("No adapter found");
            //     }
            //     ASHA::AdapterState::BluetoothOff => {
            //         ui.heading("Bluetooth is off");
            //     }
            //     ASHA::AdapterState::Okay         => {
            //         ui.heading("Searching...");
            //         // self.core.start_scan(1);
            //     }
            //     _                                => {}
            // }
        });
    }
}


#[tokio::main(flavor = "current_thread")]
async fn main() {
    let state = ASHA::ASHA::get_adapter_state().await;

    let mut test = ASHA::ASHA::new().await;

    test.start_scan(1).await;

    // tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    // let devs = test.get_discovered();
    // test.stop_scan();

    // let app = App{
    //     asha: ASHA::ASHA::new().await
    // };

    // run_native(
    //     Box::new(app),
    //     NativeOptions{
    //         maximized: true,
    //         ..Default::default()
    //     }
    // );
}