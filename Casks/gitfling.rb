cask "gitfling" do
  version "0.1.0"
  sha256 "e2243fcaa3d2b23746ead886b55a720db1436cedf057faaf6150270f91e80fae"

  url "https://github.com/kanda-ab-net/gitfling/releases/download/v#{version}/GitFling_#{version}_aarch64.dmg"
  name "GitFling"
  desc "git-ftp compatible GUI for deploying local Git changes over FTP/FTPS/SFTP"
  homepage "https://github.com/kanda-ab-net/gitfling"

  depends_on macos: ">= :big_sur"
  depends_on arch: :arm64

  app "GitFling.app"

  zap trash: [
    "~/Library/Application Support/gitfling",
    "~/Library/Saved Application State/com.gitfling.app.savedState",
  ]
end
