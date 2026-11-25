use crate::player::controller::{Player, player};
use block2::RcBlock;
use futures::{StreamExt, channel::mpsc};
use gpui_kit::{App, Context};
use objc2::runtime::AnyObject;
use objc2_foundation::{NSDictionary, NSString};
use objc2_media_player::{
    MPMediaItemPropertyTitle, MPNowPlayingInfoCenter, MPNowPlayingPlaybackState, MPRemoteCommand,
    MPRemoteCommandCenter, MPRemoteCommandEvent, MPRemoteCommandHandlerStatus,
};
use std::ptr::NonNull;

#[derive(Clone, Copy)]
enum MediaKey {
    Play,
    Pause,
    Toggle,
    Previous,
    Next,
}

fn queue_destination(key: MediaKey, position: Option<usize>, len: usize) -> Option<usize> {
    let position = position.filter(|&position| position < len)?;
    match key {
        MediaKey::Previous => position.checked_sub(1),
        MediaKey::Next => Some(position + 1).filter(|&next| next < len),
        _ => None,
    }
}

fn handle_key(key: MediaKey, player: &mut Player, cx: &mut Context<Player>) {
    if player.video.is_none() || player.error.is_some() || player.init_error.is_some() {
        return;
    }
    match key {
        MediaKey::Play if player.paused || player.eof => player.toggle_pause(),
        MediaKey::Pause if !player.paused && !player.eof => player.set_paused(true),
        MediaKey::Toggle => player.toggle_pause(),
        MediaKey::Previous | MediaKey::Next => {
            if let Some(index) = queue_destination(key, player.queue_pos, player.queue.len()) {
                player.play_queue_index(index, cx);
            }
        }
        _ => return,
    }
    cx.notify();
}

fn register(command: &MPRemoteCommand, key: MediaKey, sender: mpsc::UnboundedSender<MediaKey>) {
    // MediaPlayer can call from outside GPUI's thread. The block only sends a value.
    let handler = RcBlock::new(move |_: NonNull<MPRemoteCommandEvent>| {
        if sender.unbounded_send(key).is_ok() {
            MPRemoteCommandHandlerStatus::Success
        } else {
            MPRemoteCommandHandlerStatus::CommandFailed
        }
    });
    // SAFETY: the framework copies the block and keeps it registered for this process.
    unsafe {
        command.setEnabled(false);
        command.addTargetWithHandler(&handler);
    }
}

pub fn init(cx: &mut App) {
    let (sender, mut receiver) = mpsc::unbounded();
    // SAFETY: GPUI calls this on the main thread, once for the life of the app.
    let (now_playing, commands) = unsafe {
        let center = MPRemoteCommandCenter::sharedCommandCenter();
        let commands = [
            (center.playCommand(), MediaKey::Play),
            (center.pauseCommand(), MediaKey::Pause),
            (center.togglePlayPauseCommand(), MediaKey::Toggle),
            (center.previousTrackCommand(), MediaKey::Previous),
            (center.nextTrackCommand(), MediaKey::Next),
        ];
        (MPNowPlayingInfoCenter::defaultCenter(), commands)
    };
    for (command, key) in &commands {
        register(command, *key, sender.clone());
    }

    let player = player(cx);
    let mut title = None::<String>;
    let mut last_state = MPNowPlayingPlaybackState::Stopped;
    let mut last_enabled = [false; 5];
    cx.observe(&player, move |player, cx| {
        let player = player.read(cx);
        let video = player.video.as_ref().filter(|_| player.error.is_none() && player.init_error.is_none());
        let current_title = video.map(|video| video.title.as_str());
        let state = match video {
            None => MPNowPlayingPlaybackState::Stopped,
            Some(_) if player.paused || player.eof || player.loading => MPNowPlayingPlaybackState::Paused,
            Some(_) => MPNowPlayingPlaybackState::Playing,
        };
        // SAFETY: updates run on GPUI's main thread and the dictionary owns its values.
        unsafe {
            if title.as_deref() != current_title {
                title = current_title.map(str::to_owned);
                let info = current_title.map(|title| {
                    let value = NSString::from_str(title);
                    NSDictionary::<NSString, AnyObject>::from_slices(&[MPMediaItemPropertyTitle], &[&value])
                });
                now_playing.setNowPlayingInfo(info.as_deref());
            }
            if last_state != state {
                now_playing.setPlaybackState(state);
                last_state = state;
            }
            for (index, (command, key)) in commands.iter().enumerate() {
                let enabled = video.is_some() && match key {
                    MediaKey::Previous | MediaKey::Next => queue_destination(*key, player.queue_pos, player.queue.len()).is_some(),
                    _ => true,
                };
                if last_enabled[index] != enabled {
                    command.setEnabled(enabled);
                    last_enabled[index] = enabled;
                }
            }
        }
    }).detach();

    cx.spawn(async move |cx| {
        while let Some(key) = receiver.next().await {
            let _ = cx.update(|cx| player.update(cx, |player, cx| handle_key(key, player, cx)));
        }
    }).detach();
}

#[cfg(test)]
mod tests {
    use super::{MediaKey, queue_destination};

    #[test]
    fn track_keys_respect_queue_boundaries() {
        assert_eq!(queue_destination(MediaKey::Previous, Some(1), 3), Some(0));
        assert_eq!(queue_destination(MediaKey::Next, Some(1), 3), Some(2));
        assert_eq!(queue_destination(MediaKey::Previous, Some(0), 3), None);
        assert_eq!(queue_destination(MediaKey::Next, Some(2), 3), None);
        assert_eq!(queue_destination(MediaKey::Next, None, 0), None);
        assert_eq!(queue_destination(MediaKey::Previous, Some(9), 3), None);
    }
}
