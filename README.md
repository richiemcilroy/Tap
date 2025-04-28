# Tap is a blazing fast, GPUI powered note-taking app.

### Erm… why?
This is a [Cap](https://cap.so/) team internal hackathon project. We wanted to experiment with GPUI, so this is what I chose to build.

### How is it fast?
[**GPUI**](https://www.gpui.rs/) is a minimalist Rust UI layer that talks straight to `wgpu`, so every pixel is drawn by your graphics card instead of the CPU.

* 💨 **Micro-latency redraws** — the GPU’s thousands of cores handle text layout and rendering in parallel.  
* 🔋 **Lower battery / CPU burn**  
* 🪶 **Tiny footprint** — a native Rust binary (a few MB) instead of a 200 MB Electron bundle.

_No hidden browser, no heavyweight framework. Just Rust, `wgpu`, and your GPU doing what it does best._

## Demo


https://github.com/user-attachments/assets/cee48891-0876-487c-98c7-895f943ff292

## Features

- [x] Use **SQLite** for local storage  
- [x] Implement multi-tab support  
- [ ] Basic rich-text formatting (bold, italics, code)  
- [ ] Search
- [ ] Settings modal (theme toggle & font size)  

### How to run

```bash
# Install rust (if you don’t have it)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Grab the code
git clone https://github.com/richiemcilroy/Tap.git
cd Tap

# Start the app
cargo run            # Add --release for an optimised build
