use super::*;

impl App {
    pub(super) fn redraw(&mut self) {
        self.last_ui = Instant::now();
        self.polls();
        if self.occluded {
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(r) = &mut self.renderer else { return };
        if window.inner_size().width == 0 || window.inner_size().height == 0 {
            return;
        }
        let input = r.egui_state.take_egui_input(&window);
        let ctx = r.egui_ctx.clone();
        let image_id = r.image_id;
        ctx.begin_pass(input);
        #[allow(deprecated)]
        egui::CentralPanel::default().show(&ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10., 8.);
            ui.horizontal_wrapped(|ui| {
                ui.heading("Shader lab");
                ui.separator();
                ui.add_enabled_ui(self.bench.is_none(), |ui| {
                    egui::ComboBox::from_id_salt("catalog")
                        .selected_text(&self.selected.title)
                        .width(200.)
                        .show_ui(ui, |ui| {
                            for e in CATALOG {
                                if ui
                                    .selectable_label(self.selected.title == e.title, e.title)
                                    .clicked()
                                {
                                    self.load(Document::example(e));
                                }
                            }
                        });
                    if ui.button("Open file").clicked() {
                        let file = rfd::FileDialog::new()
                            .add_filter("Shader", &["glsl", "frag", "wgsl", "hlsl"])
                            .pick_file();
                        self.stats.clear();
                        if let Some(p) = file {
                            self.open(&p);
                        }
                    }
                    if ui.button("Paste and run").clicked() {
                        match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                            Ok(text) => self.load(Document::clipboard(&text)),
                            Err(e) => self.error(e.to_string()),
                        }
                    }
                });
                if ui
                    .add_enabled(self.active.is_some(), egui::Button::new("View code"))
                    .clicked()
                {
                    self.inspector.open = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Advanced").clicked() {
                        self.advanced = !self.advanced;
                    }
                });
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if let Some(a) = &self.active {
                    ui.strong(&a.doc.title);
                    ui.weak(format!(
                        "{} · {} {}  {}",
                        a.doc.language.to_uppercase(),
                        a.passes.len(),
                        if a.passes.len() == 1 {
                            "pass"
                        } else {
                            "passes"
                        },
                        a.doc.credit
                    ));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Fullscreen").clicked() {
                        window.set_fullscreen(if window.fullscreen().is_some() {
                            None
                        } else {
                            Some(Fullscreen::Borderless(None))
                        });
                    }
                    if ui
                        .add_enabled(self.bench.is_none(), egui::Button::new("Restart"))
                        .clicked()
                    {
                        self.rewind(true);
                    }
                    if ui
                        .add_enabled(
                            self.bench.is_none(),
                            egui::Button::new(if self.playing { "Pause" } else { "Play" }),
                        )
                        .clicked()
                    {
                        self.toggle_play();
                    }
                });
            });
            let avail = ui.available_size();
            let reserved = if self.failed { 112. } else { 76. };
            let ratio = self.fixed.map_or(16. / 9., |s| s[0] as f32 / s[1] as f32);
            let w = avail.x.min(((avail.y - reserved).max(16.)) * ratio).max(1.);
            let (area, response) = ui.allocate_exact_size(
                egui::vec2(avail.x, (w / ratio).max(1.)),
                egui::Sense::click_and_drag(),
            );
            self.preview = egui::Rect::from_center_size(area.center(), egui::vec2(w, w / ratio));
            ui.painter()
                .rect_filled(area, 2., egui::Color32::from_rgb(8, 11, 15));
            ui.painter().image(
                image_id,
                self.preview,
                egui::Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(1., 1.)),
                egui::Color32::WHITE,
            );
            if response.clicked() {
                response.request_focus();
            }
            let r = self.renderer.as_ref().unwrap();
            ui.horizontal_wrapped(|ui| {
                let stats = self.stats.cached.as_ref();
                let list = [
                    ("Rendered FPS", stats.map(|s| format!("{:.0}", s.avg_fps()))),
                    ("Frame time", stats.map(|s| format!("{:.2} ms", s.avg_ms))),
                    ("p95 frame", stats.map(|s| format!("{:.2} ms", s.p95_ms))),
                    (
                        "Render pixels",
                        Some(format!("{} × {}", r.size[0], r.size[1])),
                    ),
                    ("Backend", Some(format!("{:?}", r.info.backend))),
                    (
                        "Shader GPU",
                        r.timer
                            .as_ref()
                            .and_then(|t| t.latest())
                            .filter(|_| self.playing)
                            .map(|t| format!("{:.2} ms", t.pass_ms.iter().sum::<f64>())),
                    ),
                ];
                for (label, value) in list {
                    ui.vertical(|ui| {
                        ui.small(label);
                        ui.monospace(value.unwrap_or_else(|| "—".into()));
                    });
                    ui.separator();
                }
            });
            egui::ScrollArea::vertical().max_height(62.).show(ui, |ui| {
                if self.failed {
                    ui.colored_label(egui::Color32::from_rgb(250, 160, 145), &self.message);
                } else {
                    ui.weak(&self.message);
                }
            });
        });
        self.draw_advanced(&ctx);
        self.draw_inspector(&ctx);
        let output = ctx.end_pass();
        let pixels = ctx.pixels_per_point() * self.scale;
        let size = if self.bench.is_some() {
            self.bench_size
        } else {
            self.fixed
                .map(|s| s.map(|n| ((n as f32 * self.scale).round() as u32).max(1)))
                .unwrap_or([
                    ((self.preview.width() * pixels).round() as u32).max(1),
                    ((self.preview.height() * pixels).round() as u32).max(1),
                ])
        };
        if self.renderer.as_ref().unwrap().size != size {
            self.renderer.as_mut().unwrap().resize(size);
            self.rewind(false);
        }
        let mut interval = None;
        let mut gpu = Vec::new();
        let ready = self.renderer.as_ref().is_some_and(Renderer::has_shader)
            && !(self.loading && self.bench.is_some());
        if ready && (self.playing || self.needs_frame || self.bench.is_some()) {
            let at = Instant::now();
            let fixed = self.capture_at.is_some() || self.bench.is_some();
            let dt = if fixed {
                1. / 60.
            } else {
                self.stats.last.map_or(0., |t| (at - t).as_secs_f64())
            };
            if self.frame > 0 {
                self.time += dt;
            }
            let date = if fixed {
                [2026., 0., 1., 0.]
            } else {
                let d = chrono::Local::now();
                [
                    d.year() as f32,
                    d.month0() as f32,
                    d.day() as f32,
                    d.num_seconds_from_midnight() as f32 + d.nanosecond() as f32 / 1e9,
                ]
            };
            let r = self.renderer.as_mut().unwrap();
            let u = self.manual.take().unwrap_or(FrameInput {
                time: self.time as f32,
                delta: dt as f32,
                frame: self.frame as i32,
                date,
            });
            gpu = r.draw(
                u.time,
                u.delta,
                u.frame,
                if fixed { [0.; 4] } else { self.mouse },
                u.date,
                if fixed { &[0; 3072] } else { &self.keys },
            );
            self.keys[1024..2048].fill(0);
            self.stats.cpu = at.elapsed().as_secs_f64() * 1000.;
            interval = self.stats.frame(at);
            self.needs_frame = false;
            self.frame += 1;
            if self.stats.first == 0. {
                self.stats.first = self.started.elapsed().as_secs_f64() * 1000.;
                event(
                    "FirstFrame",
                    json!({"shader":self.active.as_ref().unwrap().doc.title,"ms":self.stats.first,"size":r.size}),
                );
            }
        }
        if ready && (self.screenshot || self.capture_at == Some(self.frame)) {
            self.screenshot = false;
            match self.renderer.as_ref().unwrap().screenshot().and_then(|p| {
                if let Some(dst) = &self.capture_path {
                    fs::copy(&p, dst).map_err(|e| e.to_string())?;
                    Ok(dst.clone())
                } else {
                    Ok(p)
                }
            }) {
                Ok(p) => {
                    self.message = format!("Saved {}", p.display());
                    event("ScreenshotTaken", json!({"path":p,"frame":self.frame}));
                }
                Err(e) => self.error(e),
            }
            if self.capture_at.is_some() {
                self.quit = true;
            }
        }
        self.renderer.as_mut().unwrap().paint(&window, output);
        if self.auto_bench && self.bench.is_none() && self.saved.is_none() && self.frame >= 3 {
            self.start_bench();
        }
        self.drive_bench(interval, gpu);
    }
    fn draw_inspector(&mut self, ctx: &egui::Context) {
        if !self.inspector.open {
            return;
        }
        let Some(a) = &self.active else { return };
        let mut open = true;
        egui::Window::new("Shader source")
            .open(&mut open)
            .default_size([760., 470.])
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.inspector.hlsl, false, "Original");
                    ui.selectable_value(&mut self.inspector.hlsl, true, "HLSL");
                    egui::ComboBox::from_id_salt("source-pass")
                        .selected_text(&a.passes[self.inspector.pass].name)
                        .show_ui(ui, |ui| {
                            for (i, p) in a.passes.iter().enumerate() {
                                ui.selectable_value(&mut self.inspector.pass, i, &p.name);
                            }
                        });
                });
                let generated = self.inspector.hlsl && a.doc.language != "hlsl";
                if generated && self.inspector.exports.is_none() && self.inspector.error.is_none() {
                    match a
                        .prepared
                        .iter()
                        .map(shader_core::hlsl)
                        .collect::<Result<Vec<_>, _>>()
                    {
                        Ok(p) => self.inspector.exports = Some(p),
                        Err(e) => self.inspector.error = Some(e),
                    }
                }
                if generated {
                    if let Some(e) = &self.inspector.error {
                        ui.label(e);
                        return;
                    }
                }
                let index = self.inspector.pass;
                let source = if generated {
                    &self.inspector.exports.as_ref().unwrap()[index]
                } else {
                    &a.passes[index].source
                };
                ui.horizontal(|ui| {
                    if ui.button("Copy pass").clicked() {
                        ctx.copy_text(source.to_string());
                    }
                    if ui.button("Copy shader").clicked() {
                        let text = if generated {
                            let mut passes = a.passes.clone();
                            for (p, src) in passes
                                .iter_mut()
                                .zip(self.inspector.exports.as_ref().unwrap())
                            {
                                p.source = src.clone();
                            }
                            shader_core::serialize_passes(&passes)
                        } else {
                            a.doc.source.to_string()
                        };
                        ctx.copy_text(text);
                    }
                });
                let lines: Vec<_> = source.lines().collect();
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show_rows(ui, 16., lines.len(), |ui, rows| {
                        for i in rows {
                            ui.monospace(lines[i]);
                        }
                    });
            });
        self.inspector.open = open;
    }
    fn draw_advanced(&mut self, ctx: &egui::Context) {
        if !self.advanced {
            return;
        }
        let mut open = true;
        egui::Window::new("Advanced")
            .open(&mut open)
            .default_width(460.)
            .show(ctx, |ui| {
                let mut next = self.settings.clone();
                let r = self.renderer.as_ref().unwrap();
                ui.label(format!(
                    "{} · {:?} · {:?}",
                    r.info.name, r.info.backend, r.config.present_mode
                ));
                ui.add_enabled_ui(self.bench.is_none(), |ui| {
                    egui::ComboBox::from_label("Backend")
                        .selected_text(next.backend.label())
                        .show_ui(ui, |ui| {
                            for b in BackendChoice::ui_choices() {
                                ui.selectable_value(&mut next.backend, *b, b.label());
                            }
                        });
                    egui::ComboBox::from_label("Adapter")
                        .selected_text(next.adapter_name.as_deref().unwrap_or("Auto"))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut next.adapter_name, None, "Auto");
                            for name in &r.available_adapters {
                                ui.selectable_value(
                                    &mut next.adapter_name,
                                    Some(name.clone()),
                                    name,
                                );
                            }
                        });
                    egui::ComboBox::from_label("Present mode")
                        .selected_text(next.present_mode.label())
                        .show_ui(ui, |ui| {
                            for m in PresentModeChoice::ui_choices() {
                                if m.to_wgpu()
                                    .is_none_or(|mode| r.present_modes.contains(&mode))
                                {
                                    ui.selectable_value(&mut next.present_mode, *m, m.label());
                                }
                            }
                        });
                    ui.add(egui::Slider::new(&mut next.frame_latency, 1..=4).text("Frame latency"));
                    if r.info.backend == wgpu::Backend::Dx12 {
                        egui::ComboBox::from_label("DX12 compiler")
                            .selected_text(next.dx12_compiler.label())
                            .show_ui(ui, |ui| {
                                for c in DxCompilerChoice::ui_choices() {
                                    ui.selectable_value(&mut next.dx12_compiler, *c, c.label());
                                }
                            });
                    }
                    ui.horizontal(|ui| {
                        ui.label("Render size");
                        for (s, label) in [
                            (None, "Fit"),
                            (Some([1280, 720]), "720p"),
                            (Some([1920, 1080]), "1080p"),
                        ] {
                            ui.selectable_value(&mut self.fixed, s, label);
                        }
                    });
                    {
                        ui.horizontal(|ui| {
                            ui.label("Render scale");
                            for (scale, label) in [(0.5, "50%"), (0.75, "75%"), (1., "100%")] {
                                ui.selectable_value(&mut self.scale, scale, label);
                            }
                        });
                    }
                });
                ui.monospace(format!(
                    "CPU submission {:.2} ms   Compile {:.1} ms   First frame {:.1} ms",
                    self.stats.cpu, self.stats.compiled, self.stats.first
                ));
                for (name, ms) in r.timed_passes() {
                    ui.monospace(format!("{name}  {ms:.3} ms"));
                }
                if r.timer.is_none() {
                    ui.label("GPU timestamps unavailable");
                }
                if next != self.settings {
                    if next.requires_renderer_rebuild(&self.settings) {
                        self.pending = Some(next);
                    } else {
                        let r = self.renderer.as_mut().unwrap();
                        r.config.present_mode = next.resolve_present_mode(&r.present_modes);
                        r.config.desired_maximum_frame_latency = next.frame_latency;
                        r.surface.configure(&r.device, &r.config);
                        self.settings = next;
                        self.stats.clear();
                    }
                }
                if ui.button("Save preview PNG").clicked() {
                    self.screenshot = true;
                }
                ui.separator();
                if let Some(b) = &self.bench {
                    ui.label(b.status_line());
                } else {
                    ui.add(egui::Slider::new(&mut self.warm, 0.5..=30.).text("Warmup seconds"));
                    ui.add(egui::Slider::new(&mut self.measure, 1.0..=60.).text("Measure seconds"));
                    ui.checkbox(&mut self.uncapped, "Request uncapped presentation");
                    if ui.button("Compare available backends").clicked() {
                        self.start_bench();
                    }
                    if ui.button("Open benchmark JSON").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Benchmark", &["json"])
                            .pick_file()
                        {
                            match bench::load_sweep(&path) {
                                Ok(s) => self.saved = Some(s),
                                Err(e) => self.message = e,
                            }
                        }
                    }
                }
                if let Some(s) = &self.saved {
                    bench::render_table(ui, &s.runs);
                }
            });
        self.advanced = open;
    }
}
