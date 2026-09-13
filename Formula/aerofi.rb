class Aerofi < Formula
  desc "Lightweight, keyboard- and mouse-driven script launcher for macOS"
  homepage "https://github.com/frostymur/aerofi"
  url "https://github.com/frostymur/aerofi/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "28b46202104f1a1cdae3dd32a6cae5673de2a5efcc11af2b06f407524b83ebe0"
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
