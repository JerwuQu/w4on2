#pragma once

#include <stdint.h>

// WASM-4 defined values
#define W4ON2_WASM4_VOLUME_MAX 100

// Limits
#define W4ON2_TRACK_COUNT 16
#define W4ON2_CHANNEL_COUNT 4
#define W4ON2_MAX_NOTES 8
#define W4ON2_MAX_PATTERNS 256

// Volumes
#define W4ON2_VOLUME_MAX 255
#define W4ON2_SUSTAIN_MAX 255
#define W4ON2_VELOCITY_MAX 127

// -----
// protospan.js format definition
#define W4ON2_FMT_LONG_DELTA_ARG2_ID 0x00 // [UpperBits][LowerBits]
#define W4ON2_FMT_LONG_DELTA_SIZE 3
#define W4ON2_FMT_LONG_DELTA_NOTES_OFF_ARG2_ID 0x01 // [UpperBits][LowerBits]
#define W4ON2_FMT_LONG_DELTA_NOTES_OFF_SIZE 3
#define W4ON2_FMT_SHORT_DELTA_ID 0x02
#define W4ON2_FMT_SHORT_DELTA_SIZE 1
#define W4ON2_FMT_SHORT_DELTA_2_START W4ON2_FMT_SHORT_DELTA_ID
#define W4ON2_FMT_SHORT_DELTA_2_COUNT 50
#define W4ON2_FMT_SHORT_DELTA_NOTES_OFF_ID 0x34
#define W4ON2_FMT_SHORT_DELTA_NOTES_OFF_SIZE 1
#define W4ON2_FMT_SHORT_DELTA_NOTES_OFF_3_START W4ON2_FMT_SHORT_DELTA_NOTES_OFF_ID
#define W4ON2_FMT_SHORT_DELTA_NOTES_OFF_3_COUNT 50
#define W4ON2_FMT_NOTE_ON_ID 0x66
#define W4ON2_FMT_NOTE_ON_SIZE 1
#define W4ON2_FMT_NOTE_ON_4_START W4ON2_FMT_NOTE_ON_ID
#define W4ON2_FMT_NOTE_ON_4_COUNT 128
#define W4ON2_FMT_NOTES_OFF_ID 0xe6
#define W4ON2_FMT_NOTES_OFF_SIZE 1
#define W4ON2_FMT_SET_FLAGS_ARG1_ID 0xe7 // [WASM-4 `flags`]
#define W4ON2_FMT_SET_FLAGS_SIZE 2
#define W4ON2_FMT_SET_VOLUME_ARG1_ID 0xe8 // [Volume]
#define W4ON2_FMT_SET_VOLUME_SIZE 2
#define W4ON2_FMT_SET_PAN_ID 0xe9
#define W4ON2_FMT_SET_PAN_SIZE 1
#define W4ON2_FMT_SET_PAN_8_START W4ON2_FMT_SET_PAN_ID
#define W4ON2_FMT_SET_PAN_8_COUNT 3
#define W4ON2_FMT_SET_VELOCITY_ARG1_ID 0xec // [Velocity]
#define W4ON2_FMT_SET_VELOCITY_SIZE 2
#define W4ON2_FMT_SET_ADSR_ARG4_ID 0xed // [A][D][S][R]
#define W4ON2_FMT_SET_ADSR_SIZE 5
#define W4ON2_FMT_SET_A_ARG1_ID 0xee // [A]
#define W4ON2_FMT_SET_A_SIZE 2
#define W4ON2_FMT_SET_D_ARG1_ID 0xef // [D]
#define W4ON2_FMT_SET_D_SIZE 2
#define W4ON2_FMT_SET_S_ARG1_ID 0xf0 // [S]
#define W4ON2_FMT_SET_S_SIZE 2
#define W4ON2_FMT_SET_R_ARG1_ID 0xf1 // [R]
#define W4ON2_FMT_SET_R_SIZE 2
#define W4ON2_FMT_SET_PITCH_ENV_ARG2_ID 0xf2 // [NoteOffset][Duration]
#define W4ON2_FMT_SET_PITCH_ENV_SIZE 3
#define W4ON2_FMT_SET_ARP_RATE_ARG1_ID 0xf3 // [Rate]
#define W4ON2_FMT_SET_ARP_RATE_SIZE 2
#define W4ON2_FMT_SET_PORTAMENTO_ARG1_ID 0xf4 // [Portamento]
#define W4ON2_FMT_SET_PORTAMENTO_SIZE 2
#define W4ON2_FMT_SET_VIBRATO_ARG2_ID 0xf5 // [Speed][Depth]
#define W4ON2_FMT_SET_VIBRATO_SIZE 3
#define W4ON2_FMT_RESERVED 0xf6
// Unused values: 9
// -----

typedef void (*w4on2_tone_t)(uint32_t frequency, uint32_t duration, uint32_t volume, uint32_t flags, void *userdata);

typedef struct {
    uint8_t flags; // channel, duty, pan according to WASM-4
    uint8_t volume;
    uint8_t velocity;
    uint8_t a, d, s, r;
    int8_t pe_offset;
    uint8_t pe_duration;
    uint8_t arp_rate;
    uint8_t portamento;
    uint8_t vib_speed, vib_depth;
} w4on2_track_t;

typedef struct {
    uint16_t first_trigger_ticks; // reset on completely new note (i.e. if active_key_count was 0)
    uint8_t last_trigger_ticks; // reset when a new note/key is triggered
    uint8_t active_track_i;
    uint8_t active_key_count;
    uint8_t note_keys[W4ON2_MAX_NOTES]; // all active notes (primarily for arpeggio)
} w4on2_channel_t;

typedef struct {
    w4on2_tone_t tone;
    void *userdata;
    w4on2_track_t tracks[W4ON2_TRACK_COUNT];
    w4on2_channel_t channels[W4ON2_CHANNEL_COUNT];
} w4on2_rt_t;

// Initialize the runtime with the given `tone` function.
void w4on2_rt_init(w4on2_rt_t *rt, w4on2_tone_t tone, void *userdata);
// Should be called every tick for continous audio playback.
void w4on2_rt_tick(w4on2_rt_t *rt);
// Manually feed an event to the runtime. Should not be used by most users.
uint8_t w4on2_rt_feed_event(w4on2_rt_t *rt, uint8_t track_i, const uint8_t *data);

typedef struct {
    uint16_t outer_data_i; // index into data
    uint16_t inner_data_i; // index into current pattern data
    uint16_t delay; // delay until next event
} w4on2_player_track_t;

typedef struct {
    const uint8_t *data;
    w4on2_player_track_t tracks[W4ON2_TRACK_COUNT];
} w4on2_player_t;

// Initialize the player with the given w4on2 binary.
void w4on2_player_init(w4on2_player_t *p, const uint8_t *data);
// Tick the player. Should usually be called before `w4on2_rt_tick`.
// Returns the amount of still active tracks, meaning it will return 0 when finished playing.
uint8_t w4on2_player_tick(w4on2_player_t *p, w4on2_rt_t *rt);
