<p align="center">
  <img src="assets/mimic-crown.svg" width="130" height="130" alt="MimicOS Crown" />
</p>

<h1 align="center">👑 Mimic</h1>

<p align="center">
  <b>A drop-in package manager for Arch Linux.</b>
</p>

<p align="center">
  <a href="https://www.gnu.org/licenses/gpl-3.0"><img src="https://img.shields.io/badge/License-GPLv3-blue.svg" alt="License: GPL v3" /></a>
  <a href="https://archlinux.org"><img src="https://img.shields.io/badge/Arch%20Linux-Package%20Engine-1793d1?logo=archlinux&logoColor=white" alt="Arch Linux" /></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/Rust-2021%20Edition-dea584?logo=rust&logoColor=white" alt="Rust" /></a>
</p>

---

## ✨ What Mimic Does

Mimic is a drop-in package manager for Arch Linux that combines official repository management, AUR package building, and direct Git compilation into a single, high-performance toolchain.

### 🛡️ Hermetic Bubblewrap Container Builds
- **Zero-Root Build Isolation**: Package compilation happens inside an unprivileged [Bubblewrap (`bwrap`)](https://github.com/containers/bubblewrap) container using `--unshare-user` and security namespaces.
- Build scripts never touch or pollute your host root filesystem.
- Staging happens in an isolated container tree (`/staging`), verified before any package is produced.

### ⚡ Battle-Tested `MemoryGuard` Concurrency
- **Dynamic Thread Budgeting**: Inspects real-time physical memory and budgets safe parallel jobs:
  $$\text{safe\_jobs} = \left(\frac{\text{RAM}_{\text{MiB}}}{1536}\right)\text{.clamp}(1, \text{nproc} - 1)$$
- On a 5.6 GiB system, Mimic automatically caps jobs to **2–3 compiler threads**, keeping at least 9 CPU hardware threads and gigabytes of memory free for your display server, compositor, and browser.
- Enforces safe concurrency across **Ninja**, **CMake**, **GNU Make**, and **Cargo**.

### 💽 NVMe Workspace Isolation (Zero `tmpfs` Bloat)
- Building multi-gigabyte C++ and Rust projects inside `/tmp` (which is often a `tmpfs` RAM-disk) drains available system memory before compilation even finishes.
- Mimic relocates build trees, git clones, staging folders, and compiler scratch caches to NVMe storage at **`/var/tmp/mimic-build`**.
- In-container `/tmp` and `/var/tmp` are bind-mounted to NVMe scratch directories, ensuring **zero RAM-disk bloat**.

### 📉 Adaptive Optimization & LTO Cliff Protection
- Link-Time Optimization (`-flto`) can cause 10+ GiB linker memory spikes on template-heavy codebases.
- **Adaptive Flags**: On machines with $\le 8\text{ GiB}$ RAM, Mimic automatically throttles to `-march=native -O2 -pipe -fno-plt` with LTO stripped. On machines with $> 8\text{ GiB}$, it unlocks full `-O3 -flto=auto`.
- Integrates `sccache` compilation caching and `mold` modern linking out of the box.

### 🛑 Kernel PSI Memory Pressure Watcher
- Mimic runs a background sentinel monitoring Linux Kernel Pressure Stall Information (`/proc/pressure/memory`).
- If memory pressure (`some avg10`) climbs past **25%**, the sentinel immediately freezes the sandbox compiler process group with `SIGSTOP`.
- Once the kernel has reclaimed pagecache and stabilized memory, Mimic cleanly resumes workers with `SIGCONT`—preventing the kernel OOM killer from ever firing.

### 🐙 Universal Git Source Runner (`mimic git`)
- Point Mimic directly at any Git repository (`mimic git https://github.com/foo/bar`).
- Automatically detects toolchains: **Cargo (Rust)**, **Meson / Ninja**, **CMake**, or **GNU Make**.
- Builds hermetically in the sandbox, stages artifacts, generates Arch `.PKGINFO` and `.BUILDINFO` metadata, archives into a zstandard `.pkg.tar.zst`, and commits it directly to your system via native ALPM.

### 🧠 Zero-RAM AI Mentor (`mimic-brain`)
- A decoupled Unix domain socket daemon (`/run/mimic/brain.sock`).
- Consumes **0 MB idle RAM** when dormant (powered by systemd socket activation).
- Wakes up on demand if a build fails to parse compiler errors (missing C headers, uninstalled pkg-config dependencies, missing CMake modules) and suggest immediate fixes.
- Provides package explanations and ArchWiki insights (`mimic why <pkg>`).

---

## 🏗️ Architecture

```
mimic-core/
├── assets/             # MimicOS vector artwork and crown icon
├── crates/
│   ├── mimic-core/     # Primary CLI, ALPM engine, bwrap sandbox builder, MemoryGuard, PSI watcher
│   └── mimic-brain/    # Decoupled AI diagnostic mentor & socket IPC triage engine
├── packaging/          # Systemd socket & service units for zero-RAM triage
├── PKGBUILD            # Arch Linux package recipe
└── task.md             # Milestone & architectural specification tracker
```

---

## 📦 Installation

### Prerequisites & Base Dependencies
Ensure the following packages are installed on your Arch Linux or Arch-based system:

```bash
sudo pacman -S --needed base-devel git rust bubblewrap sccache mold pacman openssl
```

### Option A: Build & Install from Source (Recommended)

```bash
# 1. Clone the repository
git clone https://github.com/MimicLotus/mimic-core.git
cd mimic-core

# 2. Build the optimized release binaries
cargo build --release --locked --workspace

# 3. Install binaries to system path
sudo install -Dm755 target/release/mimic /usr/bin/mimic
sudo install -Dm755 target/release/mimic-brain /usr/bin/mimic-brain

# 4. (Optional) Enable the zero-RAM AI mentor socket
sudo install -Dm644 packaging/mimic-brain.socket /usr/lib/systemd/system/mimic-brain.socket
sudo install -Dm644 packaging/mimic-brain.service /usr/lib/systemd/system/mimic-brain.service
sudo systemctl daemon-reload
sudo systemctl enable --now mimic-brain.socket
```

### Option B: Build via `makepkg`

```bash
cd mimic-core
makepkg -si
```

---

## 🚀 Usage Guide

### 1. Synchronize & Upgrade
Sync official mirrors, enabled micro-repos, and upgrade your system:
```bash
mimic sync
# or force refresh:
mimic sync --refresh
```

### 2. Search Packages
Search across official Arch repositories, GitHub micro-repos, and the AUR simultaneously:
```bash
mimic search neovim
# search only the AUR:
mimic search --aur hyprland-git
```

### 3. Install Packages
Install official binary packages or build and install from the AUR:
```bash
mimic install ripgrep
mimic install visual-studio-code-bin
```

### 4. Hermetic AUR Package Build
Build an AUR package inside an isolated Bubblewrap sandbox without installing it immediately:
```bash
mimic build paru
```
The output `.pkg.tar.zst` will be staged and placed in `/var/cache/mimic/pkg/` (or `~/.cache/mimic/pkg/`).

### 5. Build Directly from Git Source
Clone, compile, package, and install any upstream project directly from Git:
```bash
mimic git https://github.com/noctalia-dev/umbriel --noconfirm
```

### 6. Sandbox Root Testing (Safe Mode)
Want to test package installation in an isolated sandbox without altering your host system? Use `--root`:
```bash
mimic --root /var/tmp/my-sandbox git https://github.com/noctalia-dev/umbriel --noconfirm
```

### 7. Consult the AI Mentor (`mimic why`)
Ask about the purpose of any package or system library:
```bash
mimic why libxkbcommon
```

### 8. Remove Packages
Remove packages with optional cascade dependency pruning:
```bash
mimic remove unneeded-pkg --cascade
```

---

## 🤝 Philosophy & License

Software should be reliable, transparent, and respectful of your machine. Mimic is free and open-source software licensed under the **GNU General Public License v3.0 or later** ([GPL-3.0-or-later](LICENSE)).

Contributions, suggestions, and feedback are welcome! Feel free to open an issue or pull request on [GitHub](https://github.com/MimicLotus/mimic-core).

*Crafted with care for Arch Linux by Mimic Lotus.*
