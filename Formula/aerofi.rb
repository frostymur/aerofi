class Aerofi < Formula
  desc "Lightweight, keyboard- and mouse-driven script launcher for macOS"
  homepage "https://github.com/frostymur/aerofi"
  url "https://github.com/frostymur/aerofi/archive/refs/tags/v0.1.3.tar.gz"
  sha256 "abfc8993be3a13db8f88ff5ddd11f98cc591fa7b08fe6661938d116fe34205e3"
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
