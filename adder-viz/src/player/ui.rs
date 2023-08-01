use crossbeam_channel::{bounded, Receiver};
use std::error::Error;
use std::path::PathBuf;
use std::time::Duration;

use adder_codec_rs::transcoder::source::video::FramedViewMode;
use bevy::asset::Assets;
use bevy::ecs::system::Resource;
use bevy::prelude::{Commands, Image, Res, ResMut};

use bevy::time::Time;
use bevy_egui::egui::{Color32, RichText, Ui};

use crate::player::adder::{AdderPlayer, PlayerStreamArtifact, StreamState};
use crate::{add_checkbox_row, add_radio_row, add_slider_row, Images};
use bevy_egui::egui;

use rayon::current_num_threads;

#[derive(PartialEq)]
pub struct PlayerUiSliders {
    playback_speed: f32,
    thread_count: usize,
}

impl Default for PlayerUiSliders {
    fn default() -> Self {
        Self {
            playback_speed: 1.0,
            thread_count: 4,
        }
    }
}

#[derive(PartialEq, Clone)]
pub enum ReconstructionMethod {
    Fast,
    Accurate,
}

impl Default for ReconstructionMethod {
    fn default() -> Self {
        Self::Accurate
    }
}

pub struct PlayerUiState {
    playing: bool,
    looping: bool,
    view_mode: FramedViewMode,
    reconstruction_method: ReconstructionMethod,
    current_frame: u32,
    total_frames: u32,
    current_time: f32,
    total_time: f32,
    ui_sliders: PlayerUiSliders,
    ui_sliders_drag: PlayerUiSliders,
}

impl Default for PlayerUiState {
    fn default() -> Self {
        Self {
            playing: true,
            looping: true,
            view_mode: FramedViewMode::Intensity,
            reconstruction_method: ReconstructionMethod::Accurate,
            current_frame: 0,
            total_frames: 0,
            current_time: 0.0,
            total_time: 0.0,
            ui_sliders: Default::default(),
            ui_sliders_drag: Default::default(),
        }
    }
}

pub struct InfoUiState {
    stream_state: StreamState,
    events_per_sec: f64,
    events_ppc_per_sec: f64,
    events_ppc_total: f64,
    events_total: u64,
    source_name: RichText,
}

impl Default for InfoUiState {
    fn default() -> Self {
        InfoUiState {
            stream_state: Default::default(),
            events_per_sec: 0.,
            events_ppc_per_sec: 0.,
            events_ppc_total: 0.0,
            events_total: 0,
            source_name: RichText::new("No file selected yet"),
        }
    }
}

impl InfoUiState {
    fn _clear_stats(&mut self) {
        self.events_per_sec = 0.;
        self.events_ppc_per_sec = 0.;
        self.events_ppc_total = 0.0;
        self.events_total = 0;
    }
}

#[derive(Resource, Default)]
pub struct PlayerState {
    player_rx: Option<Receiver<PlayerStreamArtifact>>,
    player_path_buf: Option<PathBuf>,
    ui_state: PlayerUiState,
    pub(crate) ui_info_state: InfoUiState,
}

unsafe impl Sync for PlayerState {}

impl PlayerState {
    pub fn consume_source(
        &mut self,
        mut images: ResMut<Assets<Image>>,
        mut handles: ResMut<Images>,
    ) -> Result<(), Box<dyn Error>> {
        if !self.ui_state.playing {
            return Ok(());
        }
        if let Some(rx) = &self.player_rx {
            let (event_count, stream_state, image_opt) = rx.try_recv()?;
            self.ui_info_state.events_total += event_count;
            self.ui_info_state.stream_state = stream_state;

            if let Some(image) = image_opt {
                let handle = images.add(image);
                handles.last_image_view = handles.image_view.clone();
                handles.image_view = handle;
            }
            return Ok(());
        }
        Err("".into())
    }

    pub fn play(&mut self) {
        self.ui_state.playing = true;
    }

    // Fill in the side panel with sliders for playback speed and buttons for play/pause/stop
    pub fn side_panel_ui(
        &mut self,
        ui: &mut Ui,
        mut commands: Commands,
        _images: &mut ResMut<Assets<Image>>,
    ) {
        ui.horizontal(|ui| {
            ui.heading("ADΔER Parameters");
            if ui.add(egui::Button::new("Reset params")).clicked() {
                self.ui_state = Default::default();
                self.ui_state.ui_sliders = Default::default();
                if self.ui_state.ui_sliders_drag != self.ui_state.ui_sliders {
                    self.reset_update_adder_params(true)
                }
                self.ui_state.ui_sliders_drag = Default::default();
            }
            if ui.add(egui::Button::new("Reset video")).clicked() {
                // self.player = AdderPlayer::default();
                self.ui_state = Default::default();
                self.ui_state.ui_sliders = Default::default();
                self.ui_state.ui_sliders_drag = Default::default();
                self.ui_info_state = Default::default();
                self.reset_update_adder_params(false);
                commands.insert_resource(Images::default());
            }
        });
        egui::Grid::new("my_grid")
            .num_columns(2)
            .spacing([10.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                self.side_panel_grid_contents(ui);
            });
    }

    pub fn side_panel_grid_contents(&mut self, ui: &mut Ui) {
        let mut need_to_update = add_slider_row(
            true,
            true,
            "Playback speed:",
            ui,
            &mut self.ui_state.ui_sliders.playback_speed,
            &mut self.ui_state.ui_sliders_drag.playback_speed,
            0.1..=10000.0,
            vec![1.0, 2.0, 5.0, 10.0],
            0.1,
        );

        // TODO!
        // match &self.player.input_stream {
        //     None => {}
        //     Some(stream) => {
        //         let duration = Duration::from_nanos(
        //             ((self.player.current_t_ticks as f64 / stream.tps as f64) * 1.0e9) as u64,
        //         );
        //         ui.add_enabled(true, egui::Label::new("Current time:"));
        //         ui.add_enabled(true, egui::Label::new(to_string(duration)));
        //         ui.end_row();
        //     }
        // }

        ui.add_enabled(true, egui::Label::new("Playback controls:"));
        ui.horizontal(|ui| {
            if self.ui_state.playing {
                if ui.button("⏸").clicked() {
                    self.ui_state.playing = false;
                }
            } else if ui.button("▶").clicked() {
                self.ui_state.playing = true;
            }
            // TODO: remove this?
            if ui.button("⏹").clicked() {
                self.ui_state.playing = false;
                need_to_update = true;
            }

            if ui.button("⏮").clicked() {
                self.ui_state.playing = true;
                self.ui_info_state.stream_state.file_pos = 0; // To force the player to restart
                need_to_update = true;
            }
        });
        ui.end_row();

        // TODO: decoding is single-threaded for now
        add_slider_row(
            false,
            false,
            "Thread count:",
            ui,
            &mut self.ui_state.ui_sliders.thread_count,
            &mut self.ui_state.ui_sliders_drag.thread_count,
            1..=(current_num_threads() - 1).max(4),
            vec![],
            1,
        );
        add_checkbox_row(
            true,
            "Loop:",
            "Loop playback?",
            ui,
            &mut self.ui_state.looping,
        ); // TODO: add more sliders

        // TODO
        need_to_update |= add_radio_row(
            true,
            "View mode:",
            vec![
                ("Intensity", FramedViewMode::Intensity),
                ("D", FramedViewMode::D),
                ("Δt", FramedViewMode::DeltaT),
            ],
            ui,
            &mut self.ui_state.view_mode,
        );
        need_to_update |= add_radio_row(
            true,
            "Reconstruction method:",
            vec![
                ("Fast", ReconstructionMethod::Fast),
                ("Accurate", ReconstructionMethod::Accurate),
            ],
            ui,
            &mut self.ui_state.reconstruction_method,
        );

        if need_to_update {
            self.reset_update_adder_params(true)
        }
    }

    pub fn central_panel_ui(&mut self, ui: &mut Ui, time: Res<Time>) {
        ui.horizontal(|ui| {
            if ui.button("Open file").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("adder video", &["adder"])
                    .pick_file()
                {
                    self.player_path_buf = Some(path.clone());
                    self.replace_player(&path);
                }
            }

            ui.label("OR drag and drop your ADΔER file here (.adder)");
        });

        ui.label(self.ui_info_state.source_name.clone());

        let duration_secs = self.ui_info_state.stream_state.current_t_ticks as f64
            / self.ui_info_state.stream_state.tps as f64;
        self.ui_info_state.events_per_sec = self.ui_info_state.events_total as f64 / duration_secs;
        self.ui_info_state.events_ppc_total =
            self.ui_info_state.events_total as f64 / self.ui_info_state.stream_state.volume as f64;
        self.ui_info_state.events_ppc_per_sec = self.ui_info_state.events_ppc_total / duration_secs;

        // TODO: make fps accurate and meaningful here
        ui.label(format!(
            "{:.2} transcoded FPS\t\
            {:.2} events per source sec\t\
            {:.2} events PPC per source sec\t\
            {:.0} events total\t\
            {:.0} events PPC total
            ",
            1. / time.delta_seconds(),
            self.ui_info_state.events_per_sec,
            self.ui_info_state.events_ppc_per_sec,
            self.ui_info_state.events_total,
            self.ui_info_state.events_ppc_total
        ));
    }

    fn reset_update_adder_params(&mut self, replace_player: bool) {
        self.ui_state.current_frame = match self.ui_state.reconstruction_method {
            ReconstructionMethod::Fast => 1,
            ReconstructionMethod::Accurate => 0,
        };
        self.ui_state.total_frames = 0;
        self.ui_state.current_time = 0.0;
        self.ui_state.total_time = 0.0;

        let path_buf = match &self.player_path_buf {
            None => {
                return;
            }
            Some(p) => p.clone(),
        };

        if replace_player {
            self.replace_player(&path_buf);
        } else {
            self.player_path_buf = None;
            self.player_rx = None;
        }
    }

    pub fn replace_player(&mut self, path_buf: &std::path::Path) {
        self.player_path_buf = Some(PathBuf::from(path_buf.clone()));
        self.ui_info_state.events_total = 0;
        let mut player = match AdderPlayer::new(
            path_buf,
            self.ui_state.ui_sliders.playback_speed,
            self.ui_state.view_mode,
        ) {
            Ok(player) => {
                self.ui_info_state.source_name = RichText::from(match path_buf.to_str() {
                    None => "Error: couldn't get path string".to_string(),
                    Some(path) => path.to_string(),
                })
                .color(Color32::DARK_GREEN);
                player
            }
            Err(e) => {
                self.ui_info_state.source_name = RichText::new(e.to_string()).color(Color32::RED);
                return;
            }
        };

        player = player.reconstruction_method(self.ui_state.reconstruction_method.clone());
        player = player.stream_pos(self.ui_info_state.stream_state.file_pos);

        self.ui_state.current_frame = 1;

        let (player_tx, player_rx) = bounded(60);

        rayon::spawn(move || loop {
            println!("About to consume!");
            let res = player.consume_source();
            println!("Consumed!");
            match player_tx.send(res) {
                Ok(_) => {}
                Err(_) => {
                    break;
                }
            };
        });

        self.player_rx = Some(player_rx);
    }
}

fn _to_string(duration: Duration) -> String {
    let hours = duration.as_secs() / 3600;
    let mins = (duration.as_secs() % 3600) / 60;
    let secs = duration.as_secs() % 60;
    let nanos = duration.subsec_nanos();
    format!("{hours}:{mins}:{secs}.{nanos:09}")
}
