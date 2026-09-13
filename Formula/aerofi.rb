class Aerofi < Formula
  desc "Lightweight, keyboard- and mouse-driven script launcher for macOS"
  homepage "https://github.com/frostymur/aerofi"
  url "https://github.com/frostymur/aerofi/archive/refs/tags/v0.0.7.tar.gz"
  sha256 "190a8e4c3ffc3ab70a8108c4e514034e0e53498c61a64f05362747069377e3fc"
  license "MIT"
  head "https://github.com/frostymur/aerofi.git", branch: "main"

  depends_on "rust" => :build
  depends_on :macos

  def install
    system "cargo", "install", *std_cargo_args
  end

  service do
    run opt_bin/"aerofi"
    keep_alive true
    process_type :interactive
  end

  test do
    assert_path_exists bin/"aerofi"
  end
end
