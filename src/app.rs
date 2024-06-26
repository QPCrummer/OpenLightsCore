use std::cmp::PartialEq;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::sync::mpsc::Sender;
use std::time::Duration;

use egui::{Align, CentralPanel, Context, FontFamily, FontId, Layout, ProgressBar, RichText, ScrollArea, TextStyle, Ui, Vec2};
use egui::scroll_area::ScrollBarVisibility;

use crate::audio_player::{AudioPlayer, gather_songs_from_path, get_atomic_float, Song, start_worker_thread};
use crate::constants;
use crate::constants::{AudioThreadActions, PLAYLIST_DIRECTORY};

#[derive(PartialEq, Default)]
enum Screen {
    #[default]
    Playlist,
    Jukebox,
    FileManager,
}

pub struct OpenLightsCore {
    playlist: String,
    current_screen: Screen,
    file_explorer: FileExplorer,
    pub audio_player: Arc<AudioPlayer>,
    messenger: Sender<AudioThreadActions>,
    volume: f32,
}

impl Default for OpenLightsCore {
    fn default() -> Self {
        let audio_player = Arc::new(AudioPlayer::new());

        let (tx, rx) = mpsc::channel();
        // TODO Actually make good threading improvements
        start_worker_thread(&Arc::clone(&audio_player), rx);

        Self {
            playlist: String::from(""),
            current_screen: Screen::default(),
            file_explorer: FileExplorer::new(),
            audio_player,
            messenger: tx,
            volume: 100.0,
        }
    }
}

#[inline]
fn heading2() -> TextStyle {
    TextStyle::Name("Heading2".into())
}

#[inline]
fn heading3() -> TextStyle {
    TextStyle::Name("ContextHeading".into())
}

fn configure_text_styles(ctx: &Context) {
    use FontFamily::Proportional;
    use TextStyle::*;

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (Heading, FontId::new(100.0, Proportional)),
        (heading2(), FontId::new(30.0, Proportional)),
        (heading3(), FontId::new(20.0, Proportional)),
        (Body, FontId::new(18.0, Proportional)),
        (Monospace, FontId::new(14.0, Proportional)),
        (Button, FontId::new(14.0, Proportional)),
        (Small, FontId::new(10.0, Proportional)),
    ]
        .into();
    ctx.set_style(style);
}

impl OpenLightsCore {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.
        configure_text_styles(&cc.egui_ctx);

        Default::default()
    }

    fn show_playlist_screen(&mut self, ctx: &Context) {

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            let _ = &self.top_menu(ui);
        });

        CentralPanel::default().show(ctx, |ui| {
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.add(
                    egui::Image::new(egui::include_image!("../assets/open_lights.png"))
                        .max_width(200.0)
                        .rounding(10.0),
                );

                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Select a Playlist").text_style(heading2()).strong());
                });

                ScrollArea::vertical()
                    .auto_shrink([true, true])
                    .max_height(200.)
                    .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        for option in &self.audio_player.playlist_vec {
                            if ui.add(egui::SelectableLabel::new(
                                &self.playlist == option,
                                option,
                            )).clicked() {
                                self.playlist = String::from(option);
                            };
                            ui.add_space(10.);
                        }
                    });

                ui.add_space(30.);
                if ui.add_sized([210., 80.], egui::Button::new("Confirm")).clicked() && self.playlist != *"" {
                    self.audio_player.load_songs_from_playlist(&self.playlist);
                    self.current_screen = Screen::Jukebox;
                };
            });

            ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label("Version ");
                    ui.label(constants::VERSION);
                });
            });
        });
    }

    fn top_menu(&mut self, ui: &mut Ui) {
        egui::menu::bar(ui, |ui| {
            egui::widgets::global_dark_light_mode_buttons(ui);

            if ui.button("Playlists").clicked() {
                self.current_screen = Screen::Playlist
            }

            if ui.button("Song Manager").clicked() {
                self.current_screen = Screen::FileManager
            }
        });
    }

    fn show_jukebox_screen(&mut self, ctx: &Context) {
        ctx.request_repaint_after(Duration::from_millis(500));

        // Menu Bar
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            self.top_menu(ui);
        });

        // TODO Determine if this is safe (it should be)
        let current_song = self.audio_player.get_current_song();
        let song_vec = self.audio_player.song_vec.clone();

        // Center
        CentralPanel::default().show(ctx, |ui| {
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                // Playlist
                ui.label(RichText::new("  Playlist  ").text_style(heading2()).strong().underline());

                ScrollArea::vertical()
                    .auto_shrink([true, true])
                    .max_height(200.)
                    .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        for song in &song_vec.clone() {
                            if ui.add(egui::SelectableLabel::new(
                                &current_song == song,
                                format!("{} by {}", song.name, song.artist),
                            )).clicked() {
                                // TODO Determine if this is safe (it should be)
                                self.audio_player.song_override(song);
                            };
                            ui.add_space(10.);
                        }
                    });
            });
        });

        egui::TopBottomPanel::bottom("bottom_taskbar").show(ctx, |ui| {
            ui.set_height(150.0);

            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.separator();

                let song = self.audio_player.get_current_song();

                // Song Title
                ui.label(format!("{} by {}", song.name, song.artist));

                // Loading Bar
                Self::centered_song_progress_display(self, ui);

                // Buttons
                Self::centered_buttons(self, ui);

                // Slider
                Self::centered_volume_slider(self, ui);

            });
        });
    }

    fn show_file_manager_screen(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            self.top_menu(ui);
        });
        CentralPanel::default().show(ctx, |ui| {
            self.file_explorer.render(ui);
        });
    }

    fn centered_song_progress_display(&mut self, ui: &mut Ui) {
        let bar = ProgressBar::new(get_atomic_float(&self.audio_player.progress)).animate(false);

        // Get the width of the text to center it
        let text = format!("{} / {}", Self::format_time(Self::milliseconds_to_seconds(self.audio_player.millisecond_position.load(Ordering::Relaxed))), Self::format_time(get_atomic_float(&self.audio_player.song_duration) as i32));

        // Layout the progress bar
        ui.vertical_centered(|ui| {
            // Add the progress bar
            let response = ui.add(bar);

            // Calculate the position to center the text
            let rect = response.rect;
            let text_pos = egui::pos2(
                rect.center().x,
                rect.center().y,
            );

            // Paint the centered text
            ui.painter().text(
                text_pos,
                egui::Align2::CENTER_CENTER,
                text,
                FontId::proportional(12.),
                ui.style().visuals.text_color(),
            );
        });
    }

    fn format_time(seconds: i32) -> String {
        let minutes = seconds / 60;
        let remaining_seconds = seconds % 60;
        format!("{:02}:{:02}", minutes, remaining_seconds)
    }
    fn milliseconds_to_seconds(ms: u64) -> i32 {
        (ms / 1000) as i32
    }

    fn centered_buttons(&mut self, ui: &mut Ui) {
        let button_count = 5;
        let button_size = Vec2::new(40.0, 40.0); // Width and height of each button
        let button_spacing = ui.spacing().item_spacing.x;
        let total_button_width = button_count as f32 * button_size.x + (button_count as f32 - 1.0) * button_spacing;

        // Add space to the left to center the buttons
        let available_width = ui.available_width();
        let left_padding = (available_width - total_button_width) / 2.0;

        ui.horizontal(|ui| {
            ui.add_space(left_padding);

            if ui.add_sized(button_size, egui::Button::new("⏭")).clicked() {
                self.messenger.send(AudioThreadActions::Skip).unwrap();
            }

            if ui.add_sized(button_size, egui::Button::new("⏪")).clicked() {
                self.messenger.send(AudioThreadActions::Rewind).unwrap();
            }


            if ui.add_sized(button_size, egui::Button::new(if self.audio_player.playing.load(Ordering::Relaxed) { "⏸" } else { "▶" })).clicked() {
                if self.audio_player.playing.load(Ordering::Relaxed)  {
                    self.messenger.send(AudioThreadActions::Pause).unwrap();
                } else {
                    self.messenger.send(AudioThreadActions::Play).unwrap();
                }
            }

            if ui.add_sized(button_size, egui::Button::new("🔀")).clicked() {
                self.messenger.send(AudioThreadActions::Shuffle).unwrap();
            }

            if ui.add_sized(button_size, egui::SelectableLabel::new(self.audio_player.looping.load(Ordering::Relaxed), "🔁")).clicked() {
                self.messenger.send(AudioThreadActions::Loop).unwrap();
            }
        });
    }

    fn centered_volume_slider(&mut self, ui: &mut Ui) {
        let slider_width = Vec2::new(200., 50.);

        if ui.add_sized(slider_width, egui::Slider::new(&mut self.volume, 0.0..=100.).text("Volume").suffix("%")).drag_stopped {
            self.audio_player.set_volume((self.volume) / 100.0);
        }
    }
}

impl eframe::App for OpenLightsCore {
    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        match self.current_screen {
            Screen::Playlist => self.show_playlist_screen(ctx),
            Screen::Jukebox => self.show_jukebox_screen(ctx),
            Screen::FileManager => self.show_file_manager_screen(ctx),
        }
    }
}

#[derive(PartialEq, Default)]
enum Selection {
    #[default]
    Playlist,
    Song,
}

struct FileExplorer {
    selection: Selection,
    playlists: Vec<PathBuf>,
    songs: Vec<Song>,
    selected_index: usize,
    show_edit_buttons: bool,
}

impl FileExplorer {
    fn new() -> Self {
        let playlists = Self::read_directory((&PLAYLIST_DIRECTORY).as_ref()).unwrap_or_else(|_| vec![]);
        Self {
            selection: Selection::Playlist,
            playlists,
            songs: Vec::new(),
            selected_index: 0,
            show_edit_buttons: false,
        }
    }

    fn read_directory(path: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
        let mut entries = vec![];
        for entry in fs::read_dir(path)? {
            let entry = entry?.path();
            entries.push(entry);
        }
        Ok(entries)
    }

    fn render(&mut self, ui: &mut Ui) {
        CentralPanel::default().show(ui.ctx(), |ui| {
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                // Playlist
                ui.label(RichText::new("  Playlist  ").text_style(heading2()).strong().underline());

                ScrollArea::vertical()
                    .auto_shrink([true, true])
                    .max_height(200.)
                    .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        match self.selection {
                            Selection::Playlist => {
                                self.render_playlists(ui);
                            }
                            Selection::Song => {
                                self.render_songs(ui);
                            }
                        }
                    });

                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    if self.show_edit_buttons {
                        if ui.add_sized(Vec2::new(70.0, 20.0), egui::Button::new("Delete")).clicked() {
                            self.remove_current_selected();
                        }
                    }
                    if self.selection == Selection::Song {
                        if ui.add_sized(Vec2::new(90.0, 20.0), egui::Button::new("Playlists")).clicked() {
                            self.selected_index = 0;
                            self.show_edit_buttons = false;
                            self.selection = Selection::Playlist;
                        }
                    }
                });
            });
        });
    }

    fn render_playlists(&mut self, ui: &mut Ui) {
        for (index, path) in self.playlists.iter().enumerate() {
            let label = ui.add(egui::SelectableLabel::new(
                index == self.selected_index,
                format!("{}", path.file_stem().unwrap().to_string_lossy().into_owned().replace('_', " ")),
            ));

            if label.clicked() {
                self.show_edit_buttons = true;
                self.selected_index = index;
            }

            if label.double_clicked() {
                self.selection = Selection::Song;
                self.selected_index = 0;
                self.songs = gather_songs_from_path(&path.to_string_lossy().into_owned());
            }
            ui.add_space(10.);
        }
    }

    fn render_songs(&mut self, ui: &mut Ui) {
        for (index, song) in self.songs.iter().enumerate() {
            if ui.add(egui::SelectableLabel::new(
                index == self.selected_index,
                format!("{} by {}", song.name, song.artist),
            )).clicked() {
                self.show_edit_buttons = true;
                self.selected_index = index;
            };
            ui.add_space(10.);
        }
    }

    fn remove_current_selected(&mut self) {
        match self.selection {
            Selection::Playlist => {
                let path = self.playlists.get(self.selected_index).unwrap();
                fs::remove_dir_all(path).expect("Failed to delete playlist");
                self.playlists.remove(self.selected_index);
                self.selected_index = 0;
            }
            Selection::Song => {
                let song = self.songs.get(self.selected_index).unwrap();
                let path = song.path.parent().unwrap();
                fs::remove_dir_all(path).expect("Failed to delete song");
                self.songs.remove(self.selected_index);
                self.selected_index = 0;
            }
        }
    }
}
