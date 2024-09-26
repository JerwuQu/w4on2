const std = @import("std");

pub fn build(b: *std.Build) !void {
    const exe = b.addExecutable(.{
        .name = "cart",
        .root_source_file = b.path("src/main.zig"),
        .target = b.resolveTargetQuery(.{
            .cpu_arch = .wasm32,
            .os_tag = .freestanding,
        }),
        .optimize = .ReleaseSmall, // b.standardOptimizeOption(.{}),
    });
    exe.addIncludePath(b.path("../runtime/"));
    exe.addCSourceFile(.{
        .file = b.path("../runtime/w4on2.c"),
        .flags = &.{"-D__W4ON2_WASM4_TRACEF"},
    });

    exe.entry = .disabled;
    exe.root_module.export_symbol_names = &[_][]const u8{ "start", "update" };
    exe.import_memory = true;
    exe.initial_memory = 65536;
    exe.max_memory = 65536;
    exe.stack_size = 14752;

    b.installArtifact(exe);
}
