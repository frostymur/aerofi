class Aerofi < Formula
  desc "Lightweight, keyboard- and mouse-driven script launcher for macOS"
  homepage "https://github.com/frostymur/aerofi"
  url "https://github.com/frostymur/aerofi/archive/refs/tags/v0.1.2.tar.gz"
  sha256 "6c881a0d2c5ce1232df364f374000e2b2347cfc5419d4e1155c743d083b84852"
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
