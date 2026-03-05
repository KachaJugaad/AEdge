# Homebrew (Mac package manager — skip if already installed)
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Rust (for the Rust core crate — Person A's engine)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustc --version    # should print rustc 1.7x.x

# Node.js + pnpm (for contracts, bus, cloud layer)
brew install node
npm install -g pnpm
node --version     # must be >= 20
pnpm --version     # any recent

# Git (you likely have it)
git --version
