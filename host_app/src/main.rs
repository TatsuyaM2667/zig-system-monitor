use std::env;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use wasmer::{Instance, Module, Store, Value, imports};

// Zig 側とメモリ配置を完全に合わせるための構造体
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct WasmResponse {
    ptr: u32,
    len: u32,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Hyprland のソケット2 (.socket2.sock) のパスを環境変数から特定する
    // 通常は /run/user/1000/hypr/<HIS>/.socket2.sock にあります
    let xdg_runtime_dir = env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/run/user/1000"));

    let hypr_signature = env::var("HYPRLAND_INSTANCE_SIGNATURE")
        .expect("Hyprland is not running (Missing HYPRLAND_INSTANCE_SIGNATURE)");

    let socket_path = xdg_runtime_dir
        .join("hypr")
        .join(&hypr_signature)
        .join(".socket2.sock");

    println!("Connecting to Hyprland socket: {:?}", socket_path);

    // 2. Wasmer の初期化と plugin.wasm の読み込み
    let mut store = Store::default();
    // 1つ上の階層（プロジェクトルート）にある plugin.wasm を読み込む
    let wasm_bytes = std::fs::read("../plugin.wasm")
        .expect("Failed to read plugin.wasm. Did you build it in the parent directory?");
    let module = Module::new(&store, wasm_bytes)?;
    let import_object = imports! {};
    let instance = Instance::new(&mut store, &module, &import_object)?;

    // Wasm 内の関数を取り出す
    let alloc_fn = instance.exports.get_function("alloc")?;
    let free_fn = instance.exports.get_function("free")?;
    let format_fn = instance.exports.get_function("format_hyprland_event")?;
    let memory = instance.exports.get_memory("memory")?;

    // 3. Hyprland のイベントソケットに接続
    let stream = UnixStream::connect(socket_path)?;
    let reader = BufReader::new(stream);

    println!("Connected! Monitoring Hyprland events via Wasm plugin...\n");

    // 4. ソケットからリアルタイムにイベント行を読み込むループ
    for line_result in reader.lines() {
        let line = line_result?;
        if line.is_empty() {
            continue;
        }

        // --- Wasm の檻（メモリ）に文字列を流し込む処理 ---
        let input_bytes = line.as_bytes();
        let input_len = input_bytes.len() as i32;

        // Wasm 側のメモリを確保
        let alloc_args = vec![Value::I32(input_len)];
        let alloc_res = alloc_fn.call(&mut store, &alloc_args)?;
        let wasm_ptr = alloc_res[0].i32().unwrap();

        // 確保した Wasm メモリに Rust 側の文字列を書き込む
        let view = memory.view(&store);
        view.write(wasm_ptr as u64, input_bytes)?;

        // Wasm 側から戻り値（Response構造体）を書き込んでもらうための領域（8バイト分）を確保
        let ret_alloc_args = vec![Value::I32(8)];
        let ret_alloc_res = alloc_fn.call(&mut store, &ret_alloc_args)?;
        let ret_ptr = ret_alloc_res[0].i32().unwrap();

        // Wasm の関数を実行 (引数: 入力ポインタ, 入力長さ, 戻り値格納用ポインタ)
        let format_args = vec![
            Value::I32(wasm_ptr),
            Value::I32(input_len),
            Value::I32(ret_ptr),
        ];
        format_fn.call(&mut store, &format_args)?;

        // 書き込まれた Response 構造体（ポインタ 4バイト + 長さ 4バイト）を Wasm メモリから読み出す
        let mut response_buf = [0u8; 8];
        view.read(ret_ptr as u64, &mut response_buf)?;

        let res_ptr = u32::from_le_bytes(response_buf[0..4].try_into().unwrap());
        let res_len = u32::from_le_bytes(response_buf[4..8].try_into().unwrap());

        // 加工された文字列の本体を Wasm メモリから読み出して Rust の String に復元
        if res_len > 0 {
            let mut result_buf = vec![0u8; res_len as usize];
            view.read(res_ptr as u64, &mut result_buf)?;
            let formatted_str = String::from_utf8_lossy(&result_buf);

            // 画面に出力！
            println!("{}", formatted_str);
        }

        // メモリリークを防ぐため、今回使った Wasm 側のメモリを解放
        free_fn.call(
            &mut store,
            &vec![Value::I32(wasm_ptr), Value::I32(input_len)],
        )?;
        free_fn.call(&mut store, &vec![Value::I32(ret_ptr), Value::I32(8)])?;
    }

    Ok(())
}
