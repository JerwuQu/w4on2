const w4 = @import("wasm4.zig");
const C = @cImport({
    @cInclude("w4on2.h");
});
const song = @embedFile("songs/w4on2_tests.w4on2");

var runtime: C.w4on2_rt_t = undefined;
var player: C.w4on2_player_t = undefined;

fn tone(frequency: u32, duration: u32, volume: u32, flags: u32, _: ?*anyopaque) callconv(.C) void {
    w4.tone(frequency, duration, volume, flags);
}

export fn start() void {
    C.w4on2_rt_init(&runtime, tone, null);
    C.w4on2_player_init(&player, song);
}

export fn update() void {
    _ = C.w4on2_player_tick(&player, &runtime);
    C.w4on2_rt_tick(&runtime);
}
