//! Keys for 3D models: texture, normal map, wireframe, shadow, shading, projection, and turning the model about its axes.
//! They change `settings.model` (the same values as Settings > 3D) and save it, so the choice sticks.

use super::*;
use nv_core::keys::Action;

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

impl App {
    pub fn model_key(&mut self, a: Action) {
        let (textures, normal_maps) = self.meta.get(&self.id_at(self.folder.index)).map(|m| (m.textures, m.normal_maps)).unwrap_or((0, 0));
        let m = &mut self.settings.model;
        let msg = match a {
            Action::ToggleTexture => {
                m.texture = !m.texture;
                format!("Texture {}{}", on_off(m.texture), if textures == 0 { " (this model has no texture)" } else { "" })
            }
            Action::ToggleNormalMap => {
                m.normal_map = !m.normal_map;
                format!("Normal map {}{}", on_off(m.normal_map), if normal_maps == 0 { " (this model has no normal map)" } else { "" })
            }
            Action::ToggleWireframe => {
                m.wireframe = !m.wireframe;
                format!("Wireframe {}", on_off(m.wireframe))
            }
            Action::ToggleShadow => {
                m.shadow = !m.shadow;
                format!("Shadow {}", on_off(m.shadow))
            }
            Action::CycleShading => {
                m.shading = m.shading.next();
                format!("Shading: {}", m.shading.label())
            }
            Action::ToggleProjection => {
                m.ortho = !m.ortho;
                if m.ortho { "Orthographic".to_string() } else { "Perspective".to_string() }
            }
            Action::ModelRotX | Action::ModelRotY | Action::ModelRotZ => {
                let k = match a {
                    Action::ModelRotX => 0,
                    Action::ModelRotY => 1,
                    _ => 2,
                };
                let back = nv_win::modifiers().shift;
                m.turns[k] = (m.turns[k] + if back { 3 } else { 1 }) % 4;
                format!("Model turned: X {}\u{00B0}  Y {}\u{00B0}  Z {}\u{00B0}", m.turns[0] as u32 * 90, m.turns[1] as u32 * 90, m.turns[2] as u32 * 90)
            }
            _ => return,
        };
        if let Err(e) = self.settings.save() {
            self.toast(format!("Settings not saved: {e}"));
        } else {
            self.toast(msg);
        }
        invalidate(self.hwnd);
    }
}
