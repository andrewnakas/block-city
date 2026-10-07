//! Cross-mode scoring. Every mode reports what happened as a `MashEvent`;
//! the synergy rules reward doing things *across* modes (a kill mid-kickflip,
//! a carjack straight out of a web swing, a snipe through a portal).

use crate::core::modes::ActiveModes;
use bevy::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Trick,
    Bail,
    Kill { airborne: bool, headshot: bool },
    Crime,
    Carjack,
    Block,
    PortalTravel,
    SwingRelease { speed: f32 },
    Platform,
    Streak,
}

#[derive(Message, Clone, Debug)]
pub struct MashEvent {
    pub kind: Kind,
    pub label: String,
    pub points: u32,
}

impl MashEvent {
    pub fn new(kind: Kind, label: impl Into<String>, points: u32) -> Self {
        Self { kind, label: label.into(), points }
    }
}

#[derive(Clone, Debug)]
pub struct FeedLine {
    pub text: String,
    pub points: u32,
    pub age: f32,
    pub synergy: bool,
}

#[derive(Resource, Default)]
pub struct Score {
    pub total: u64,
    pub feed: Vec<FeedLine>,
    pub kills: u32,
    pub kill_streak: u32,
    last_trick: Option<(String, u32, f32)>,
    last_portal: f32,
    last_swing: Option<(f32, f32)>,
    clock: f32,
}

/// Pure synergy rules, separated for testing. Returns (label, bonus) pairs.
pub fn synergies(s: &Score, ev: &MashEvent) -> Vec<(String, u32)> {
    let mut out = vec![];
    let now = s.clock;
    match &ev.kind {
        Kind::Kill { airborne, .. } => {
            if let Some((name, pts, t)) = &s.last_trick
                && now - t < 1.5
            {
                out.push((format!("{} KILL", name.to_uppercase()), pts * 2 + 200));
            } else if *airborne {
                out.push(("AIRBORNE KILL".into(), 250));
            }
            if now - s.last_portal < 2.0 {
                out.push(("THROUGH THE PORTAL".into(), 500));
            }
        }
        Kind::Carjack => {
            if let Some((t, _)) = s.last_swing
                && now - t < 2.5
            {
                out.push(("AERIAL JACK".into(), 800));
            }
        }
        Kind::Trick => {
            if let Some((t, speed)) = s.last_swing
                && now - t < 3.0
            {
                out.push(("SWING BOOST".into(), (ev.points as f32 * speed / 20.0) as u32));
            }
            if now - s.last_portal < 2.0 {
                out.push(("PORTAL TRICK".into(), ev.points));
            }
        }
        _ => {}
    }
    out
}

pub struct ScorePlugin;

impl Plugin for ScorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Score>().add_message::<MashEvent>().add_systems(Update, tally);
    }
}

fn tally(time: Res<Time<bevy::time::Real>>, modes: Res<ActiveModes>, mut events: MessageReader<MashEvent>, mut s: ResMut<Score>) {
    let dt = time.delta_secs();
    s.clock += dt;
    // Running more modes at once multiplies everything a little.
    let mash = 1.0 + 0.1 * (modes.count().saturating_sub(1)) as f32;
    for ev in events.read() {
        let bonus = synergies(&s, ev);
        let base = (ev.points as f32 * mash) as u32;
        s.total += base as u64;
        let now = s.clock;
        match &ev.kind {
            Kind::Trick => s.last_trick = Some((ev.label.clone(), ev.points, now)),
            Kind::PortalTravel => s.last_portal = now,
            Kind::SwingRelease { speed } => s.last_swing = Some((now, *speed)),
            Kind::Kill { .. } => {
                s.kills += 1;
                s.kill_streak += 1;
            }
            Kind::Bail => s.last_trick = None,
            _ => {}
        }
        if !ev.label.is_empty() {
            s.feed.push(FeedLine { text: ev.label.clone(), points: base, age: 0.0, synergy: false });
        }
        for (label, pts) in bonus {
            let pts = (pts as f32 * mash) as u32;
            s.total += pts as u64;
            s.feed.push(FeedLine { text: label, points: pts, age: 0.0, synergy: true });
        }
    }
    for l in s.feed.iter_mut() {
        l.age += dt;
    }
    s.feed.retain(|l| l.age < 3.5);
    let n = s.feed.len();
    if n > 8 {
        s.feed.drain(0..n - 8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_right_after_trick_is_named_after_it() {
        let mut s = Score::default();
        s.clock = 10.0;
        s.last_trick = Some(("Kickflip".into(), 150, 9.5));
        let b = synergies(&s, &MashEvent::new(Kind::Kill { airborne: true, headshot: false }, "Kill", 100));
        assert_eq!(b, vec![("KICKFLIP KILL".to_string(), 500)]);
    }

    #[test]
    fn stale_trick_does_not_count() {
        let mut s = Score::default();
        s.clock = 10.0;
        s.last_trick = Some(("Kickflip".into(), 150, 5.0));
        let b = synergies(&s, &MashEvent::new(Kind::Kill { airborne: false, headshot: false }, "Kill", 100));
        assert!(b.is_empty());
    }

    #[test]
    fn swing_release_boosts_tricks_and_carjacks() {
        let mut s = Score::default();
        s.clock = 3.0;
        s.last_swing = Some((2.0, 30.0));
        let b = synergies(&s, &MashEvent::new(Kind::Trick, "Kickflip", 150));
        assert_eq!(b, vec![("SWING BOOST".to_string(), 225)]);
        let b = synergies(&s, &MashEvent::new(Kind::Carjack, "Carjack", 300));
        assert_eq!(b[0].0, "AERIAL JACK");
    }
}
