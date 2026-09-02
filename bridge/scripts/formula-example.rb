# What thrashr888/homebrew-tap generates for cider when the release carries
# cider-bridge-<version>-macos-universal.tar.gz (see release.md). Without that
# asset the resource, the resource("bridge").stage line and caveats are
# omitted and the formula is the plain binary one. The sha256 values here are
# placeholders; the real ones come from the release assets.
#
# The bridge lands at <prefix>/opt/cider/libexec/Cider Bridge.app and
# <prefix>/opt/cider/bin/cider-bridge, which is where cider looks for it.
class Cider < Formula
  desc "Manage macOS Apple apps from the command line"
  homepage "https://github.com/thrashr888/cider"
  version "0.6.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/thrashr888/cider/releases/download/v0.6.0/cider-aarch64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    else
      url "https://github.com/thrashr888/cider/releases/download/v0.6.0/cider-x86_64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  resource "bridge" do
    url "https://github.com/thrashr888/cider/releases/download/v0.6.0/cider-bridge-0.6.0-macos-universal.tar.gz"
    sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  end

  def install
    bin.install "cider"
    resource("bridge").stage do
      libexec.install "Cider Bridge.app"
      bin.install "cider-bridge"
    end
  end

  def caveats
    <<~EOS
      Cider Bridge is installed for WeatherKit, Calendar, Reminders and Contacts.
      Live HomeKit needs a personal build: cider bridge build --install
      (Xcode + Apple Developer team)
    EOS
  end

  test do
    assert_match "cider", shell_output("#{bin}/cider --help")
  end
end
