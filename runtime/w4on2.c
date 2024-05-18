#include "w4on2.h"

#ifdef __W4ON2_WASM4_TRACEF
    #define WASM_IMPORT(name) __attribute__((import_name(name)))
    __attribute__((__format__ (__printf__, 1, 2)))
    WASM_IMPORT("tracef") void tracef (const char* fmt, ...);
#else
    #ifdef __W4ON2_PRINTF_TRACEF
        #include <stdio.h>
        #define tracef(...) printf(__VA_ARGS__)
    #else
        #define tracef(...)
    #endif
#endif

static int32_t w4on2_ramp(int32_t ticks, int32_t duration, int32_t from, int32_t to)
{
    if (duration == 0 || ticks >= duration) return to;
    else if (ticks <= 0) return from;
    else return from + ((to - from) * ticks) / duration;
}
static void w4on2_ramp2add(int32_t *out1, int32_t *out2, uint32_t ticks, uint32_t duration, uint32_t from, uint32_t to)
{
    *out1 += w4on2_ramp(ticks, duration, from, to);
    *out2 += w4on2_ramp(ticks + 1, duration, from, to);
}

// phase should be 0..=0xffff
static int32_t w4on2_triangle(uint32_t phase, int32_t peak)
{
    if (phase < 0x7fff) {
        return (2 * peak * phase / 0x7fff) - peak;
    } else {
        return (2 * peak * (0xffff - phase) / 0x7fff) - peak;
    }
}

void w4on2_rt_init(w4on2_rt_t *rt, w4on2_tone_t tone, void *userdata)
{
    rt->tone = tone;
    rt->userdata = userdata;
    for (uint8_t i = 0; i < W4ON2_TRACK_COUNT; i++) {
        rt->tracks[i] = (w4on2_track_t){
            .velocity = W4ON2_VELOCITY_MAX,
            .flags = 0,
            .volume = W4ON2_VOLUME_MAX,
            .a = 0,
            .d = 0,
            .s = W4ON2_SUSTAIN_MAX,
            .r = 0,
            .pe_offset = 0,
            .pe_duration = 0,
            .arp_rate = 0,
            .portamento = 0,
            .vib_speed = 0,
            .vib_depth = 0,
        };
    }
    for (uint8_t i = 0; i < W4ON2_CHANNEL_COUNT; i++) {
        rt->channels[i] = (w4on2_channel_t){
            .active_track_i = 0xff,
            .active_key_count = 0,
            .first_trigger_ticks = 0,
            .last_trigger_ticks = 0,
        };
    }
}

void w4on2_rt_tick(w4on2_rt_t *rt)
{
    // Play each channel
    for (uint8_t ch_i = 0; ch_i < W4ON2_CHANNEL_COUNT; ch_i++) {
        w4on2_channel_t *ch = &rt->channels[ch_i];
        if (ch->active_track_i >= W4ON2_TRACK_COUNT) {
            continue;
        }
        w4on2_track_t *track = &rt->tracks[ch->active_track_i];

        // Convert volumes to WASM-4 values
        uint32_t vel_undiv = (uint32_t)track->volume * (uint32_t)track->velocity;
        uint8_t peak_amp = (W4ON2_WASM4_VOLUME_MAX * vel_undiv) / (W4ON2_VOLUME_MAX * W4ON2_VELOCITY_MAX);
        uint8_t sus_amp = (W4ON2_WASM4_VOLUME_MAX * vel_undiv * (uint32_t)track->s) / (W4ON2_VOLUME_MAX * W4ON2_VELOCITY_MAX * W4ON2_SUSTAIN_MAX);

        // Handle note
        if (ch->active_key_count > 0) {
            // Find current and last key
            // - notes: last in `ch->note_keys`
            // - arps: based on arp_rate
            uint8_t key_i = track->arp_rate > 0
                ? (ch->first_trigger_ticks / track->arp_rate) % ch->active_key_count
                : ch->active_key_count - 1;
            uint8_t key = ch->note_keys[key_i];
            uint8_t prev_key = ch->note_keys[(key_i + ch->active_key_count - 1) % ch->active_key_count];

            // ADS(R)
            // - notes: reset at the first note
            // - arps: reset with each arpeggio note
            uint16_t key_ticks = track->arp_rate > 0 && ch->active_key_count >= 2
                ? ch->first_trigger_ticks % track->arp_rate
                : ch->first_trigger_ticks;
            int32_t from_vol = 0, to_vol = 0;
            if (key_ticks < track->a) { // attack
                w4on2_ramp2add(&from_vol, &to_vol, key_ticks, track->a, 0, peak_amp);
            } else { // decay & sustain
                w4on2_ramp2add(&from_vol, &to_vol, key_ticks - track->a, track->d, peak_amp, sus_amp);
            }

            // Pitch, scaled up by 256 from MIDI notes to include bends
            int32_t from_pitch = 0, to_pitch = 0;

            // Portamento
            // - notes: porta from last to newest
            // - arps: porta between each arpeggio note
            uint16_t porta_ticks = track->arp_rate > 0
                ? key_ticks
                : ch->last_trigger_ticks;
            w4on2_ramp2add(&from_pitch, &to_pitch, porta_ticks, track->portamento, prev_key << 8, key << 8);

            // Pitch envelope
            w4on2_ramp2add(&from_pitch, &to_pitch, key_ticks, track->pe_duration, track->pe_offset << 8, 0);

            // Vibrato
            from_pitch += w4on2_triangle((0x3fff + (uint32_t)porta_ticks * ((uint32_t)track->vib_speed << 6)) & 0xffff, track->vib_depth << 2);
            to_pitch += w4on2_triangle((0x3fff + (uint32_t)(porta_ticks + 1) * ((uint32_t)track->vib_speed << 6)) % 0xffff, track->vib_depth << 2);

            // Convert from pitch to WASM-4 bent MIDI notes to WASM-4 frequency slope
            uint32_t w4_freq_param =
                ((((uint32_t)from_pitch >> 8) | ((uint32_t)from_pitch << 8)) & 0xffff)
                | (((((uint32_t)to_pitch >> 8) | ((uint32_t)to_pitch << 8)) & 0xffff) << 16);

            // Continous linear tone
            // Using the Decay part of ADSR is most flexible for playing any linear envelope since peak and sustain are absolute values in WASM-4.
            // The downside is WASM-4 defaults peak volume to 100 when it is 0, so we use Attack specifically for that case (since it goes from zero.)
            if (from_vol != 0) {
                rt->tone(
                    w4_freq_param,
                    1 << 16, // decay
                    to_vol | (from_vol << 8),
                    track->flags | 0x40,
                    rt->userdata
                );
            } else if (to_vol != 0) {
                rt->tone(
                    w4_freq_param,
                    1 << 24, // attack
                    to_vol | (to_vol << 8), // both required
                    track->flags | 0x40,
                    rt->userdata
                );
            }
        } else {
            // For Release we only trigger once and let WASM-4 handle the ramping
            if (ch->first_trigger_ticks == 0) {
                uint8_t key = ch->note_keys[0]; // last released note is placed into ch->note_keys[0]
                rt->tone(
                    key,
                    track->r << 8,
                    sus_amp,
                    track->flags | 0x40,
                    rt->userdata
                );
            }
        }

        // Tick tock - avoid wrapping
        if (ch->first_trigger_ticks < 0xffff) {
            ch->first_trigger_ticks++;
        }
        if (ch->last_trigger_ticks < 0xff) {
            ch->last_trigger_ticks++;
        }
    }
}

static uint16_t w4on2_u16be(const uint8_t *data)
{
    return (uint16_t)(data[0] << 8) | (uint16_t)data[1];
}

uint8_t w4on2_rt_feed_event(w4on2_rt_t *rt, uint8_t track_i, const uint8_t *data)
{
    w4on2_track_t *t = &rt->tracks[track_i];
    w4on2_channel_t *ch = &rt->channels[t->flags & 0x3];

    // Handle each command
    // NOTE: make sure these are in order!!
    uint8_t cmd = data[0];
    if (cmd == W4ON2_FMT_LONG_DELTA_ARG2_ID) {
        // unhandled
        return W4ON2_FMT_LONG_DELTA_SIZE;
    } else if (cmd == W4ON2_FMT_LONG_DELTA_NOTES_OFF_ARG2_ID) {
        // unhandled
        return W4ON2_FMT_LONG_DELTA_NOTES_OFF_SIZE;
    } else if (cmd < W4ON2_FMT_SHORT_DELTA_2_START + W4ON2_FMT_SHORT_DELTA_2_COUNT) {
        // unhandled
        return W4ON2_FMT_SHORT_DELTA_SIZE;
    } else if (cmd < W4ON2_FMT_SHORT_DELTA_NOTES_OFF_3_START + W4ON2_FMT_SHORT_DELTA_NOTES_OFF_3_COUNT) {
        // unhandled
        return W4ON2_FMT_SHORT_DELTA_NOTES_OFF_SIZE;
    } else if (cmd < W4ON2_FMT_NOTE_ON_4_START + W4ON2_FMT_NOTE_ON_4_COUNT) {
        // channel track switch
        if (track_i != ch->active_track_i) {
            ch->active_track_i = track_i;
            ch->active_key_count = 0;
        }
        // note overflow: push notes downwards to leave room (pop first)
        if (ch->active_key_count >= W4ON2_MAX_NOTES) {
            for (uint8_t i = 0; i < W4ON2_MAX_NOTES - 1; i++) {
                ch->note_keys[i] = ch->note_keys[i + 1];
            }
            ch->active_key_count--;
        }
        // new note
        if (ch->active_key_count == 0) {
            ch->first_trigger_ticks = 0;
        }
        // add
        ch->note_keys[ch->active_key_count++] = cmd - W4ON2_FMT_NOTE_ON_4_START;
        ch->last_trigger_ticks = 0;
        return W4ON2_FMT_NOTE_ON_SIZE;
    } else if (cmd == W4ON2_FMT_NOTES_OFF_ID) {
        if (ch->active_key_count > 0) {
            // last released note is place into ch->note_keys[0] with ch->first_trigger_ticks = 0
            uint8_t key = t->arp_rate > 0
                ? ch->note_keys[(ch->first_trigger_ticks / t->arp_rate) % ch->active_key_count]
                : ch->note_keys[ch->active_key_count - 1];
            ch->note_keys[0] = key;
            ch->active_key_count = 0;
            ch->first_trigger_ticks = 0;
        }
        return W4ON2_FMT_NOTES_OFF_SIZE;
    } else if (cmd == W4ON2_FMT_SET_FLAGS_ARG1_ID) {
        t->flags = data[1];
        return W4ON2_FMT_SET_FLAGS_SIZE;
    } else if (cmd == W4ON2_FMT_SET_VOLUME_ARG1_ID) {
        t->volume = data[1];
        return W4ON2_FMT_SET_VOLUME_SIZE;
    } else if (cmd < W4ON2_FMT_SET_PAN_8_START + W4ON2_FMT_SET_PAN_8_COUNT) {
        t->flags = ((t->flags) & ~(0x30)) | ((cmd - W4ON2_FMT_SET_PAN_8_START) << 4);
        return W4ON2_FMT_SET_PAN_SIZE;
    } else if (cmd == W4ON2_FMT_SET_VELOCITY_ARG1_ID) {
        t->velocity = data[1];
        return W4ON2_FMT_SET_VELOCITY_SIZE;
    } else if (cmd == W4ON2_FMT_SET_ADSR_ARG4_ID) {
        t->a = data[1];
        t->d = data[2];
        t->s = data[3];
        t->r = data[4];
        return W4ON2_FMT_SET_ADSR_SIZE;
    } else if (cmd == W4ON2_FMT_SET_A_ARG1_ID) {
        t->a = data[1];
        return W4ON2_FMT_SET_A_SIZE;
    } else if (cmd == W4ON2_FMT_SET_D_ARG1_ID) {
        t->d = data[1];
        return W4ON2_FMT_SET_D_SIZE;
    } else if (cmd == W4ON2_FMT_SET_S_ARG1_ID) {
        t->s = data[1];
        return W4ON2_FMT_SET_S_SIZE;
    } else if (cmd == W4ON2_FMT_SET_R_ARG1_ID) {
        t->r = data[1];
        return W4ON2_FMT_SET_R_SIZE;
    } else if (cmd == W4ON2_FMT_SET_PITCH_ENV_ARG2_ID) {
        t->pe_offset = data[1];
        t->pe_duration = data[2];
        return W4ON2_FMT_SET_PITCH_ENV_SIZE;
    } else if (cmd == W4ON2_FMT_SET_ARP_RATE_ARG1_ID) {
        t->arp_rate = data[1];
        return W4ON2_FMT_SET_ARP_RATE_SIZE;
    } else if (cmd == W4ON2_FMT_SET_PORTAMENTO_ARG1_ID) {
        t->portamento = data[1];
        return W4ON2_FMT_SET_PORTAMENTO_SIZE;
    } else if (cmd == W4ON2_FMT_SET_VIBRATO_ARG2_ID) {
        t->vib_speed = data[1];
        t->vib_depth = data[2];
        return W4ON2_FMT_SET_VIBRATO_SIZE;
    }
    return 0;
}

void w4on2_player_init(w4on2_player_t *p, const uint8_t *data)
{
    p->data = data;
    for (uint8_t track_i = 0; track_i < W4ON2_TRACK_COUNT; track_i++) {
        p->tracks[track_i] = (w4on2_player_track_t){
            .outer_data_i = 0,
            .inner_data_i = 0,
            .delay = 0,
        };
    }
}

uint8_t w4on2_player_tick(w4on2_player_t *p, w4on2_rt_t *rt)
{
    uint16_t sz = (uint16_t)(p->data[0] << 8) | (uint16_t)p->data[1];
    uint8_t pattern_count = p->data[2];
    uint8_t track_count = p->data[3];
    uint16_t first_track_offset_idx = 4 + pattern_count * 2;
    uint16_t first_track_start = w4on2_u16be(p->data + first_track_offset_idx);
    uint8_t active_tracks = 0;
    for (uint8_t track_i = 0; track_i < track_count; track_i++) {
        w4on2_player_track_t *pt = &p->tracks[track_i];
        uint16_t track_offset_idx = 4 + pattern_count * 2 + track_i * 2;
        uint16_t track_start = w4on2_u16be(p->data + track_offset_idx);
        uint16_t track_end = track_i < track_count - 1 ? w4on2_u16be(p->data + track_offset_idx + 2) : sz;

        // init track
        if (pt->outer_data_i == 0) {
            pt->outer_data_i = track_start;
        }

        // still playing?
        if (pt->outer_data_i < track_end) {
            active_tracks++;
        }

        // handle events
        while (pt->outer_data_i < track_end) {
            // get pattern
            uint8_t ptn_i = p->data[pt->outer_data_i];
            uint16_t ptn_offset_idx = 4 + ptn_i * 2;
            uint16_t ptn_start = w4on2_u16be(p->data + ptn_offset_idx);
            uint16_t ptn_end = ptn_i < pattern_count - 1 ? w4on2_u16be(p->data + ptn_offset_idx + 2) : first_track_start;
            if (pt->inner_data_i >= ptn_end) {
                // go to next pattern
                pt->inner_data_i = 0;
                pt->outer_data_i++;
                continue;
            }

            // init pattern index
            if (pt->inner_data_i == 0) {
                pt->inner_data_i = ptn_start;
            }

            // handle event
            // delays are handled specially to reduce memory usage otherwise needed for a stop flag
            uint8_t cmd = p->data[pt->inner_data_i];
            if (cmd == W4ON2_FMT_LONG_DELTA_ARG2_ID) {
                if (pt->delay == 0) {
                    pt->delay = w4on2_u16be(p->data + pt->inner_data_i + 1) + W4ON2_FMT_SHORT_DELTA_2_COUNT + 1;
                } else if (--pt->delay == 0) {
                    pt->inner_data_i += W4ON2_FMT_LONG_DELTA_SIZE;
                    continue; // continue to next event after delay
                }
                break; // break from track since we are delaying
            } else if (cmd == W4ON2_FMT_LONG_DELTA_NOTES_OFF_ARG2_ID) {
                if (pt->delay == 0) {
                    pt->delay = w4on2_u16be(p->data + pt->inner_data_i + 1) + W4ON2_FMT_SHORT_DELTA_NOTES_OFF_3_COUNT + 1;
                } else if (--pt->delay == 0) {
                    pt->inner_data_i += W4ON2_FMT_LONG_DELTA_NOTES_OFF_SIZE;
                    w4on2_rt_feed_event(rt, track_i, &(uint8_t){W4ON2_FMT_NOTES_OFF_ID});
                    continue; // continue to next event after delay
                }
                break; // break from track since we are delaying
            } else if (cmd < W4ON2_FMT_SHORT_DELTA_2_START + W4ON2_FMT_SHORT_DELTA_2_COUNT) {
                if (pt->delay == 0) {
                    pt->delay = cmd - W4ON2_FMT_SHORT_DELTA_2_START + 1;
                } else if (--pt->delay == 0) {
                    pt->inner_data_i += W4ON2_FMT_SHORT_DELTA_SIZE;
                    continue; // continue to next event after delay
                }
                break; // break from track since we are delaying
            } else if (cmd < W4ON2_FMT_SHORT_DELTA_NOTES_OFF_3_START + W4ON2_FMT_SHORT_DELTA_NOTES_OFF_3_COUNT) {
                if (pt->delay == 0) {
                    pt->delay = cmd - W4ON2_FMT_SHORT_DELTA_NOTES_OFF_3_START + 1;
                } else if (--pt->delay == 0) {
                    pt->inner_data_i += W4ON2_FMT_SHORT_DELTA_NOTES_OFF_SIZE;
                    w4on2_rt_feed_event(rt, track_i, &(uint8_t){W4ON2_FMT_NOTES_OFF_ID});
                    continue; // continue to next event after delay
                }
                break; // break from track since we are delaying
            } else {
                pt->inner_data_i += w4on2_rt_feed_event(rt, track_i, &p->data[pt->inner_data_i]);
            }
        }
    }
    return active_tracks;
}
