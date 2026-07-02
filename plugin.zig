const std = @import("std");

pub export fn alloc(size: usize) [*]u8 {
    const allocator = std.heap.page_allocator;
    const block = allocator.alloc(u8, size) catch @panic("failed to allocate memory");
    return block.ptr;
}

pub export fn free(ptr: [*]u8, size: usize) void {
    const allocator = std.heap.page_allocator;
    allocator.free(ptr[0..size]);
}

const Response = extern struct {
    ptr: [*]const u8,
    len: usize,
};

pub export fn format_hyprland_event(ptr: [*]const u8, len: usize, ret_ptr: *Response) void {
    const input = ptr[0..len];
    const allocator = std.heap.page_allocator;

    // 波括弧 {} をすべて排除し、1つの巨大な「式」として繋ぎます
    const formatted = if (std.mem.startsWith(u8, input, "activewindow>>")) blk: {
        const win_info = input["activewindow>>".len..];
        var iter = std.mem.splitScalar(u8, win_info, ',');
        const first_part = iter.first();

        // 明示的にブロック名（blk）に値を break して返すことで、Zigは絶対に迷いません
        break :blk if (std.mem.indexOf(u8, first_part, "zed") != null or std.mem.indexOf(u8, first_part, "Zed") != null)
            allocator.dupe(u8, "📝 Zed Editor | Hacking...") catch &[_]u8{}
        else if (std.mem.indexOf(u8, first_part, "firefox") != null)
            allocator.dupe(u8, "🦊 Firefox | Browsing...") catch &[_]u8{}
        else
            std.fmt.allocPrint(allocator, "🖥️  Workspace: {s}", .{first_part}) catch &[_]u8{};
    } else std.fmt.allocPrint(allocator, "{s}", .{input}) catch &[_]u8{};

    // Rust側が用意してくれたメモリ空間に結果を書き込む
    ret_ptr.* = Response{
        .ptr = formatted.ptr,
        .len = formatted.len,
    };
}
