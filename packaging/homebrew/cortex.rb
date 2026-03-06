# typed: false
# frozen_string_literal: true

# Homebrew formula for CortexAIMemory
#
# Install: brew install this-rs/tap/cortex
#
# This formula downloads pre-built binaries from GitHub Releases.
# SHA256 checksums are updated automatically by the release CI.
class Cortex < Formula
  desc "AI agent orchestrator met IndentiaGraph kennisgraaf en Tree-sitter codeanalyse"
  homepage "https://github.com/this-rs/cortex-ai-memory"
  license "MIT"
  version "VERSION_PLACEHOLDER"

  on_macos do
    on_arm do
      url "https://github.com/this-rs/cortex-ai-memory/releases/download/v#{version}/cortex-full-#{version}-macos-arm64.tar.gz"
      sha256 "SHA256_MACOS_ARM64_PLACEHOLDER"
    end

    on_intel do
      url "https://github.com/this-rs/cortex-ai-memory/releases/download/v#{version}/cortex-full-#{version}-macos-x86_64.tar.gz"
      sha256 "SHA256_MACOS_X86_64_PLACEHOLDER"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/this-rs/cortex-ai-memory/releases/download/v#{version}/cortex-full-#{version}-linux-arm64.tar.gz"
      sha256 "SHA256_LINUX_ARM64_PLACEHOLDER"
    end

    on_intel do
      url "https://github.com/this-rs/cortex-ai-memory/releases/download/v#{version}/cortex-full-#{version}-linux-x86_64.tar.gz"
      sha256 "SHA256_LINUX_X86_64_PLACEHOLDER"
    end
  end

  def install
    bin.install "cortex"
    bin.install "cortex-cli"
    bin.install "cortex-mcp"
    bin.install "cortex-mem"
    bin.install "cortex-mem-hook"

    # ONNX Runtime dylib — present only in macOS x86_64 builds (dynamic linking
    # because ort-sys has no prebuilt static library for macOS Intel).
    # Binaries have @executable_path/../lib in their rpath for this layout.
    lib.install Dir["libonnxruntime*"] unless Dir["libonnxruntime*"].empty?
  end

  def caveats
    <<~EOS
      To start the server:
        brew services start cortex
        # or: cortex serve

      To configure Claude Code integration:
        cortex setup-claude
        (auto-configures the MCP server in Claude Code)

      To start the Claude Code memory plugin:
        cortex-mem
        (captures observations and context across sessions)

      The MCP server binary is at: #{opt_bin}/cortex-mcp
      The CLI tool is at: #{opt_bin}/cortex-cli

      The default graph backend is IndentiaGraph (SurrealDB).
      Ensure SurrealDB is running before starting.
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/cortex --version")
  end

  service do
    run [opt_bin/"cortex", "serve"]
    keep_alive true
    working_dir var/"cortex"
    log_path var/"log/cortex.log"
    error_log_path var/"log/cortex-error.log"
  end
end
