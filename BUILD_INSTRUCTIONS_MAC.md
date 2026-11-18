# Building Quill on a Mac

These steps build Quill from its source code and install it in your
Applications folder. A Mac you build on yourself opens the app without the
"cannot check it for malicious software" warning that unsigned downloads show.

## What you need

- macOS 11 Big Sur or later. Quill runs on macOS 11 and later; the build
  tools below may need a newer macOS than that.
- A Mac with an Apple Silicon (M1 or newer) or Intel processor. Quill builds
  for the processor of the Mac you build on. The published downloads are made
  on an Apple Silicon Mac, for both kinds of Mac (see "Packages for both kinds
  of Mac" below). The Intel builds have not been tested on Intel hardware yet,
  and building on an Intel Mac has not been tested either.
- Several gigabytes of free disk space for the tools and the build.
- An internet connection the first time, to download the tools and the code
  libraries Quill uses.

Every command below is typed in Terminal (in Applications > Utilities). Copy
one line at a time, press Return, and wait for it to finish.

## 1. Install the Xcode Command Line Tools

These provide the compiler, linker and code signing tools.

    xcode-select --install

A window asks whether to install them; click Install and wait. If it says the
tools are already installed, go on to the next step.

## 2. Install Rust

Rust is the language Quill is written in. Install it with rustup, the official
installer:

    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

Press Return to accept the default installation. When it finishes, quit
Terminal and open it again, then check it worked:

    rustc --version

It should print a version of 1.88 or newer. If you already had Rust, bring it
up to date with `rustup update`.

## 3. Install Python 3.11 or newer

Quill's build script is written in Python. The `python3` that comes with the
Command Line Tools is too old, so check what you have:

    python3 --version

If it prints 3.11 or higher, go on to the next step. Otherwise download the
current macOS installer from https://www.python.org/downloads/macos/, run it,
then quit and reopen Terminal and check again.

## 4. Get the source code

Either download it with Git:

    git clone https://github.com/QuillWP/quill.git

or download a release's "Source code (zip)" from
https://github.com/QuillWP/quill/releases and double-click it to unzip.

Then go into the folder (adjust the path if you put it somewhere else):

    cd quill

## 5. Build Quill

    python3 tools/package.py

The first build downloads the code libraries and takes several minutes; later
builds are faster. When it finishes, the last line names the app it made:
`target/release/Quill.app`.

To also run Quill's tests before building (slower), use
`python3 tools/package.py --check` instead.

## 6. Install it

If an older Quill is open, quit it first. Then show the new app in the Finder:

    open target/release

Drag Quill into your Applications folder (choose Replace if it asks), then open
it from Applications or Launchpad as usual.

## Updating later

If you used Git, get the newest code and build again:

    git pull
    python3 tools/package.py

then install it as in step 6.

If you downloaded a zip, download the new one and repeat steps 4 to 6.

## If something goes wrong

- **"python3: command not found" or a version below 3.11:** install Python
  from python.org (step 3), then quit and reopen Terminal.
- **"cargo: command not found":** quit and reopen Terminal after installing
  Rust, or run `source "$HOME/.cargo/env"`.
- **"xcrun: error: invalid active developer path":** the Command Line Tools
  are missing; run step 1 again.
- **The build stops with an error:** run `rustup update`, then try again. If it
  still fails, [open an issue](https://github.com/QuillWP/quill/issues) and
  paste the last 30 or so lines Terminal printed.

## A debug build

For reporting a crash, a debug build gives more detail about where Quill
stopped, at the cost of running much more slowly:

    python3 tools/package.py --debug

It is made at `target/debug/Quill Debug.app`.

## Packages for both kinds of Mac

Steps 5 and 6 build only for the Mac you are using. To make the disk images
that are published, add `--dist`:

    python3 tools/package.py --dist

On a Mac this builds Quill for both Apple Silicon and Intel. It builds for your
own processor first and then cross-compiles for the other one, which needs no
extra software on the Mac, but the other processor's Rust target must be
installed once. For an Apple Silicon Mac, that is:

    rustup target add x86_64-apple-darwin

(On an Intel Mac, add `aarch64-apple-darwin` instead.) If it is missing,
`--dist` stops before building anything and prints that command.

The disk images are written to the `dist` folder, one for each processor, with
the processor in the name:

    Quill-<version>-macos-aarch64.dmg   (Apple Silicon)
    Quill-<version>-macos-x86_64.dmg    (Intel)

`dist/Quill.app` is the app for your own Mac. Add `--debug` to make the debug
versions of both. The packages are signed ad hoc, so macOS shows the warning
described in the README when someone opens them. The Intel build is made on an
Apple Silicon Mac and has not been run on Intel hardware yet.

To build for just one processor, set it yourself:

    CARGO_BUILD_TARGET=x86_64-apple-darwin python3 tools/package.py --dist
