![BongoCat](https://socialify.git.ci/ayangweb/BongoCat/image?font=JetBrains+Mono&forks=1&issues=1&language=1&logo=https%3A%2F%2Fi0.hdslb.com%2Fbfs%2Fopenplatform%2F8b37066049da62a8e6d105363472ef72fe2e435b.png&name=1&owner=1&pattern=Brick+Wall&pulls=1&stargazers=1&theme=Auto)

<div align="center">

English | [简体中文](README.zh-CN.md)

</div>

<div align="center">
  <div>
    <a href="https://github.com/ayangweb/BongoCat/releases"><img alt="Windows" src="https://img.shields.io/badge/-Windows-blue?style=flat-square&logo=data:image/svg+xml;base64,PHN2ZyB0PSIxNzI2MzA1OTcxMDA2IiBjbGFzcz0iaWNvbiIgdmlld0JveD0iMCAwIDEwMjQgMTAyNCIgdmVyc2lvbj0iMS4xIiB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHAtaWQ9IjE1NDgiIHdpZHRoPSIxMjgiIGhlaWdodD0iMTI4Ij48cGF0aCBkPSJNNTI3LjI3NTU1MTYxIDk2Ljk3MTAzMDEzdjM3My45OTIxMDY2N2g0OTQuNTEzNjE5NzVWMTUuMDI2NzU3NTN6TTUyNy4yNzU1NTE2MSA5MjguMzIzNTA4MTVsNDk0LjUxMzYxOTc1IDgwLjUyMDI4MDQ5di00NTUuNjc3NDcxNjFoLTQ5NC41MTM2MTk3NXpNNC42NzA0NTEzNiA0NzAuODMzNjgyOTdINDIyLjY3Njg1OTI1VjExMC41NjM2ODE5N2wtNDE4LjAwNjQwNzg5IDY5LjI1Nzc5NzUzek00LjY3MDQ1MTM2IDg0Ni43Njc1OTcwM0w0MjIuNjc2ODU5MjUgOTE0Ljg2MDMxMDEzVjU1My4xNjYzMTcwM0g0LjY3MDQ1MTM2eiIgcC1pZD0iMTU0OSIgZmlsbD0iI2ZmZmZmZiI+PC9wYXRoPjwvc3ZnPg==" /></a>
    <a href="https://github.com/ayangweb/BongoCat/releases"><img alt="macOS" src="https://img.shields.io/badge/-macOS-black?style=flat-square&logo=apple&logoColor=white" /></a>
  </div>

  <p>
    <a href="./LICENSE"><img src="https://img.shields.io/github/license/ayangweb/BongoCat?style=flat-square" /></a>
    <a href="https://github.com/ayangweb/BongoCat/releases/latest"><img src="https://img.shields.io/github/v/release/ayangweb/BongoCat?label=release&sort=semver&style=flat-square" /></a>
    <a href="https://github.com/ayangweb/BongoCat/releases"><img src="https://img.shields.io/github/downloads/ayangweb/BongoCat/total?style=flat-square" /></a>
  </p>

  <p>
    <a href="https://trendshift.io/developers/8507" target="_blank"><img src="https://trendshift.io/api/badge/developers/8507" alt="ayangweb | Trendshift" width="250" height="55" /></a>
    <a href="https://trendshift.io/repositories/14605" target="_blank"><img src="https://trendshift.io/api/badge/repositories/14605" alt="ayangweb%2FBongoCat | Trendshift" width="250" height="55" /></a>
    <a href="https://hellogithub.com/repository/7d23863fd4be47b39e816193ded385c9" target="_blank">
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="https://abroad.hellogithub.com/v1/widgets/recommend.svg?rid=7d23863fd4be47b39e816193ded385c9&claim_uid=5ihRVIuTYBmSGtQ&theme=dark" />
        <source media="(prefers-color-scheme: light)" srcset="https://abroad.hellogithub.com/v1/widgets/recommend.svg?rid=7d23863fd4be47b39e816193ded385c9&claim_uid=5ihRVIuTYBmSGtQ&theme=neutral" />
        <img alt="HelloGitHub" src="https://abroad.hellogithub.com/v1/widgets/recommend.svg?rid=7d23863fd4be47b39e816193ded385c9&claim_uid=5ihRVIuTYBmSGtQ&theme=neutral" width="250" height="55" />
      </picture>
    </a>
  </p>
</div>

<table width="100%">
  <thead>
    <tr>
      <th width="50%" align="center">macOS</th>
      <th width="50%" align="center">Windows</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td width="50%" align="center"><img src="https://i0.hdslb.com/bfs/openplatform/56f72568579bb44284382fe1f9b9049a700282d3.png" alt="macOS" width="100%" /></td>
      <td width="50%" align="center"><img src="https://i0.hdslb.com/bfs/openplatform/65129576aa3062f840d0640c060298c1d52b9be2.png" alt="Windows" width="100%" /></td>
    </tr>
  </tbody>
</table>

BongoCat is a desktop companion for Windows, macOS and Linux/Wayland. A Live2D cat lives on your screen, its eyes and paws follow your mouse, and it reacts to
every key, mouse button and gamepad button you press. Bring your own models, move the window
wherever you like, and it stays out of your way until you want it again.

Inspired by [Bongo-Cat-Mver](https://github.com/MMmmmoko/Bongo-Cat-Mver) by
[MMmmmoko](https://github.com/MMmmmoko).

## Features

- Runs on macOS, Windows and Linux/Wayland.
- Matches the right motion to every key, mouse button or gamepad button you press.
- Bring your own Live2D models and make the cat your own.
- Fully open source, public code, and no collection of user data.
- Works offline with no network access, so your privacy is protected.

## Linux build

Install the build and runtime dependencies on Arch Linux, then start a Production build from the
repository root:

```sh
sudo pacman -S --needed base-devel rust alsa-lib dbus libxcb libxkbcommon libxkbcommon-x11 \
  systemd-libs wayland vulkan-icd-loader xdg-desktop-portal desktop-file-utils
cargo build --locked -p bongocat-app --release --features production
desktop_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
install -d "$desktop_dir"
desktop-file-install --dir="$desktop_dir" \
  --set-key=Exec --set-value="$(realpath target/release/bongocat-app)" \
  resources/linux/com.ayangweb.bongo-cat.desktop
cargo run --locked -p bongocat-app --release --features production
```

The desktop entry must be installed before starting a source build; GLib also rejects an
application entry whose `Exec` program cannot be found, so the source build command replaces the
packaged `Exec` with the built executable's absolute path. Reference file contents:

```ini
[Desktop Entry]
Type=Application
Version=1.0
Name=BongoCat
Comment=Interactive desktop companion
Exec=/absolute/path/to/BongoCat/target/release/bongocat-app
Icon=com.ayangweb.bongo-cat
Terminal=false
Categories=Utility;
StartupNotify=true
```

A Vulkan driver for your GPU is also required, along with a desktop-specific portal backend such as
`xdg-desktop-portal-kde` or `xdg-desktop-portal-gnome`.
Global keyboard, mouse-button and relative-motion animation needs read access to the relevant
`/dev/input/event*` devices; grant that access through your distribution's device policy.
Packages should grant the required keyboard and mouse event devices through a seat-aware udev
`uaccess` rule.

On compositors that provide the Wayland layer-shell protocol, including KDE Plasma, the source build
can keep the model above normal windows and constrain its output-relative position to the screen.
Other compositors fall back to generic xdg-shell, where the corresponding controls are disabled.

Global shortcuts are registered through the XDG Desktop Portal; the desktop may show its own
confirmation surface the first time a shortcut is bound.

## More models

You can browse and download more cat models, or share your own, in
[Awesome-BongoCat](https://github.com/ayangweb/Awesome-BongoCat).

## Community

<table>
  <thead>
    <tr>
      <th>QQ Group 1</th>
      <th>QQ Group 2</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>
        <a href="https://qm.qq.com/q/AS3gNv2Vzy">
          <picture>
            <source
              media="(prefers-color-scheme: dark)"
              srcset="https://i0.hdslb.com/bfs/openplatform/8ecdc4982ab01b59d7731fcca3ec26631a274560.png"
            />
            <source
              media="(prefers-color-scheme: light)"
              srcset="https://i0.hdslb.com/bfs/openplatform/09f56580397063e1819c4c2ed63d07dee12720e1.png"
            />
            <img
              alt="QQ Group 1"
              src="https://i0.hdslb.com/bfs/openplatform/09f56580397063e1819c4c2ed63d07dee12720e1.png"
              height="250"
            />
          </picture>
        </a>
      </td>
      <td>
        <a href="https://qm.qq.com/q/TmltLAod2O">
          <picture>
            <source
              media="(prefers-color-scheme: dark)"
              srcset="https://i0.hdslb.com/bfs/openplatform/473c522487ff33e0f32b15466aeb0734f17161c8.png"
            />
            <source
              media="(prefers-color-scheme: light)"
              srcset="https://i0.hdslb.com/bfs/openplatform/d5ae8c5af6ae1d0a1f066705ee822d1287384cf6.png"
            />
            <img
              alt="QQ Group 2"
              src="https://i0.hdslb.com/bfs/openplatform/d5ae8c5af6ae1d0a1f066705ee822d1287384cf6.png"
              height="250"
            />
          </picture>
        </a>
      </td>
    </tr>
  </tbody>
</table>

## Support

Every bit of recognition is appreciated. Donations are welcome, but never expected — thank you for
loving the cat ❤️

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://i0.hdslb.com/bfs/openplatform/e7438bff14cdfb6bfd0feacbb482f99ea4093294.png" />
  <source media="(prefers-color-scheme: light)" srcset="https://i0.hdslb.com/bfs/openplatform/da55cc3ec1556580c91e59f589792866c998c7c6.png" />
  <img alt="WeChat donation code" src="https://i0.hdslb.com/bfs/openplatform/da55cc3ec1556580c91e59f589792866c998c7c6.png" height="250" />
</picture>

## Contributing

Contributions are very welcome. Please read the [contributing guide](CONTRIBUTING.md).

<a href="https://openomy.com/ayangweb/BongoCat" target="_blank" style="display: block; width: 100%;" align="center">
  <img src="https://openomy.com/svg?repo=ayangweb/BongoCat&chart=bubble" alt="Contribution Leaderboard" style="display: block; width: 100%;" />
</a>

## Star history

<a href="https://www.star-history.com/#ayangweb/BongoCat&Date">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=ayangweb/BongoCat&type=Date&theme=dark" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=ayangweb/BongoCat&type=Date" />
   <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=ayangweb/BongoCat&type=Date" />
 </picture>
</a>

## License

BongoCat is open source under the [Apache License 2.0](LICENSE) license.
