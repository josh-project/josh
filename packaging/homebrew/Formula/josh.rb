class Josh < Formula
  desc "Git projections & sync tooling for monorepos"
  homepage "https://github.com/josh-project/josh"
  version "26.08.28"
  license "MIT"

  # Prebuilt darwin binary attached to the GitHub release by rust-macos.yml.
  # Formula updates are manual: bump the `version` line and the checksum; the
  # url interpolates from `version`.
  if OS.mac? && Hardware::CPU.arm?
    url "https://github.com/josh-project/josh/releases/download/r#{version}/josh-#{version}-aarch64-apple-darwin",
        using: :nounzip
    sha256 "e8d0904222e2c129e13e4ca45616bb0f9dae43f3f8d7d7a0e051607c0f33f0e6"
  end

  livecheck do
    url :stable
    strategy :github_latest
    regex(/^r(\d+(?:\.\d+)+)$/i)
  end

  def install
    # The release asset is the bare binary; rename it into place.
    bin.install "josh-#{version}-aarch64-apple-darwin" => "josh"
  end

  test do
    system bin/"josh", "--version"
  end
end
