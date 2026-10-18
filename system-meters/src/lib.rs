//! System meters: how busy the CPU, the memory and the graphics card are. A widget for the head with three thin bars stacked
//! (and the numbers in its tooltip), the same three as small circles (a widget of its own), and a ring for each, which waits
//! in the tray of the editor.
//!
//! It reads the `system` sensor, which the island sends every 2 seconds, and answers each reading with what to draw.

use nadi_plugin::*;

const CPU: &str = "#5AC8E6";
const RAM: &str = "#B47EE6";
const GPU: &str = "#8ADB6E";
const WARM: &str = "#E6A15A";
const HOT: &str = "#E5675C";

/// the colour of a bar: its own, until the load asks for attention
fn tone(pct: f32, own: &'static str) -> &'static str {
    if pct >= 95.0 {
        HOT
    } else if pct >= 80.0 {
        WARM
    } else {
        own
    }
}

fn handle(ev: &Event) -> Reply {
    let Some(s) = (ev.event == "system").then_some(()).and(ev.system.as_ref()) else {
        return Reply::nothing();
    };
    let mut meters: Vec<(&str, f32, &'static str)> = vec![("CPU", s.cpu, CPU), ("RAM", s.ram, RAM)];
    if let Some(g) = s.gpu {
        meters.push(("GPU", g, GPU));
    }
    let bars: Vec<(&str, f32, &str, &str)> = meters.iter().map(|(l, p, c)| (*l, p / 100.0, tone(*p, c), "")).collect();
    let words: Vec<String> = meters.iter().map(|(_, p, _)| format!("{}%", p.round() as i32)).collect();
    let in_tip: Vec<(&str, f32, &str, &str)> = meters.iter().zip(&words).map(|((l, p, c), w)| (*l, p / 100.0, tone(*p, c), w.as_str())).collect();
    let tip = Blocks::new().bars(&in_tip);
    let tip_of_rings = Blocks::new().bars(&in_tip);
    let mut rings = vec![
        Ring::new("cpu", s.cpu, format!("CPU · {}%", s.cpu.round() as i32), CPU).in_tray(),
        Ring::new("ram", s.ram, format!("RAM · {}%\n{:.1} / {:.1} GB", s.ram.round() as i32, s.ram_used_gb, s.ram_total_gb), RAM).in_tray(),
    ];
    if let Some(g) = s.gpu {
        rings.push(Ring::new("gpu", g, format!("GPU · {}%", g.round() as i32), GPU).in_tray());
    }
    // (the same numbers twice: as thin bars, and as circles)
    let circles: Vec<(&str, f32, &str, &str)> = bars.iter().map(|(l, p, c, _)| (*l, *p, *c, "")).collect();
    Reply::widget("bars", Blocks::new().bars(&bars), tip).with_widget("rings", Blocks::new().rings(&circles), tip_of_rings).rings(rings)
}

plugin!(handle);

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(cpu: f32, ram: f32, gpu: Option<f32>) -> Event {
        Event { event: "system".into(), system: Some(System { cpu, ram, ram_used_gb: 9.8, ram_total_gb: 16.0, gpu }), ..Event::default() }
    }

    #[test]
    fn a_reading_draws_the_bars_the_tooltip_and_the_rings() {
        let r = handle(&reading(34.0, 61.0, Some(22.0))).to_json();
        let bars = &r["widgets"]["bars"]["blocks"][0]["bars"];
        assert_eq!(bars.as_array().unwrap().len(), 3);
        // the same three numbers, as circles
        let circles = &r["widgets"]["rings"]["blocks"][0]["rings"];
        assert_eq!(circles.as_array().unwrap().len(), 3);
        assert_eq!(circles[0]["label"], "CPU");
        assert_eq!((bars[0]["label"].as_str(), bars[0]["color"].as_str()), (Some("CPU"), Some("#5AC8E6")));
        assert_eq!(bars[1]["part"].as_f64().map(|p| (p * 100.0).round()), Some(61.0));
        let tip = &r["widgets"]["bars"]["tip"];
        assert_eq!(tip.as_array().unwrap().len(), 1);
        assert_eq!(tip[0]["bars"][1]["text"], "61%");
        let rings = r["rings"].as_array().unwrap();
        assert_eq!(rings.len(), 3);
        assert_eq!(rings[0]["start"], false);
    }

    #[test]
    fn a_busy_bar_changes_colour_and_no_graphics_counter_means_no_gpu_bar() {
        let r = handle(&reading(97.0, 85.0, None)).to_json();
        let bars = r["widgets"]["bars"]["blocks"][0]["bars"].as_array().unwrap().clone();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0]["color"], "#E5675C");
        assert_eq!(bars[1]["color"], "#E6A15A");
        assert_eq!(r["rings"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn another_kind_of_event_is_left_alone() {
        assert!(handle(&Event { event: "tick".into(), ..Event::default() }).to_json().get("widgets").is_none());
    }
}
