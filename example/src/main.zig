const w4 = @import("wasm4.zig");
const C = @cImport({
    @cInclude("w4on2.h");
});
const song = @embedFile("songs/w4on2_tests.w4on2");

var runtime: C.w4on2_rt_t = undefined;
var player: C.w4on2_player_t = undefined;

fn tone(frequency: u32, duration: u32, volume: u32, flags: u32, _: ?*anyopaque) callconv(.C) void {
    // w4.tracef(
    //     "tone %d,%d,%d,%d (%d,%d) (%d,%d,%d,%d) (%d,%d)",
    //     frequency,
    //     duration,
    //     volume,
    //     flags,
    //     (frequency >> 0) & 0xffff,
    //     (frequency >> 16) & 0xffff,
    //     (duration >> 24) & 0xff,
    //     (duration >> 16) & 0xff,
    //     (duration >> 0) & 0xff,
    //     (duration >> 8) & 0xff,
    //     (volume >> 0) & 0xff,
    //     (volume >> 8) & 0xff,
    // );
    w4.tone(frequency, duration, volume, flags);
}

export fn start() void {
    C.w4on2_rt_init(&runtime, tone, null);
    C.w4on2_player_init(&player, song);
}

export fn update() void {
    C.w4on2_player_tick(&player, &runtime);
    C.w4on2_rt_tick(&runtime);
}
