{
  description = "gnil-fm — a calm, keyboard-friendly file manager for Wayland";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      runtimeLibraries = pkgs: with pkgs; [
        acl
        bzip2
        fontconfig
        freetype
        libarchive
        libxml2
        libxkbcommon
        lz4
        openssl
        wayland
        vulkan-loader
        xz
        zlib
        zstd
      ];
      nixosModule = { config, lib, pkgs, ... }:
        let cfg = config.programs.gnil-fm; in {
          options.programs.gnil-fm = {
            enable = lib.mkEnableOption "gnil-fm Wayland file manager";
            portal.enable = lib.mkEnableOption "gnil-fm as the FileChooser portal for Niri";
            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
              defaultText = lib.literalExpression "inputs.gnil-fm.packages.${pkgs.stdenv.hostPlatform.system}.default";
              description = "gnil-fm package to install.";
            };
          };
          config = lib.mkIf cfg.enable {
            environment.systemPackages = [ cfg.package ];
            xdg.portal = lib.mkIf cfg.portal.enable {
              enable = true;
              extraPortals = [ cfg.package ];
              config.niri."org.freedesktop.impl.portal.FileChooser" = [ "gnilfm" "gtk" ];
            };
          };
        };
      homeManagerModule = { config, lib, pkgs, ... }:
        let cfg = config.programs.gnil-fm; in {
          options.programs.gnil-fm = {
            enable = lib.mkEnableOption "gnil-fm Wayland file manager";
            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
              defaultText = lib.literalExpression "inputs.gnil-fm.packages.${pkgs.stdenv.hostPlatform.system}.default";
              description = "gnil-fm package to install.";
            };
            defaultFileManager = lib.mkEnableOption "gnil-fm as the default directory handler";
            portal.enable = lib.mkEnableOption "gnil-fm as the FileChooser portal for Niri";
          };
          config = lib.mkIf cfg.enable {
            home.packages = [ cfg.package ];
            xdg.mimeApps = lib.mkIf cfg.defaultFileManager {
              enable = true;
              defaultApplications."inode/directory" = [ "gnil-fm.desktop" ];
            };
            xdg.configFile."xdg-desktop-portal/niri-portals.conf" =
              lib.mkIf cfg.portal.enable {
                text = ''
                  [preferred]
                  default=gnome;gtk;
                  org.freedesktop.impl.portal.FileChooser=gnilfm;gtk;
                '';
              };
          };
        };
    in {
      devShells = forAllSystems (system:
        let pkgs = nixpkgs.legacyPackages.${system}; in {
          default = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [
              cargo
              clang
              clippy
              cmake
              pkg-config
              rustc
              rustfmt
            ];
            buildInputs = runtimeLibraries pkgs;
            LD_LIBRARY_PATH = nixpkgs.lib.makeLibraryPath (runtimeLibraries pkgs);
            LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
            RUST_BACKTRACE = "1";
          };
        });

      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          cleanSource = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: _type:
              let name = builtins.baseNameOf path; in
              !(builtins.elem name [
                  ".git"
                  ".agents"
                  ".codex"
                  "flake.lock"
                  "flake.nix"
                  "target"
                  "result"
                ]
                || pkgs.lib.hasPrefix "result-" name
                || pkgs.lib.hasSuffix "-chroma.png" name);
          };
          gnilPackage = pkgs.rustPlatform.buildRustPackage {
            pname = "gnil-fm";
            version = "0.1.0";
            src = cleanSource;
            cargoLock.lockFile = ./Cargo.lock;
            nativeBuildInputs = [ pkgs.clang pkgs.cmake pkgs.makeWrapper pkgs.pkg-config ];
            buildInputs = runtimeLibraries pkgs;
            LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
            postInstall = ''
              install -Dm644 packaging/gnil-fm.desktop \
                $out/share/applications/gnil-fm.desktop
              install -Dm644 packaging/io.github.gnil_fm.Gnil.metainfo.xml \
                $out/share/metainfo/io.github.gnil_fm.Gnil.metainfo.xml
              install -Dm644 packaging/gnilfm.portal \
                $out/share/xdg-desktop-portal/portals/gnilfm.portal
              install -d $out/share/dbus-1/services $out/lib/systemd/user
              substitute packaging/org.freedesktop.impl.portal.desktop.gnilfm.service.in \
                $out/share/dbus-1/services/org.freedesktop.impl.portal.desktop.gnilfm.service \
                --replace-fail '@portal_executable@' "$out/bin/gnil-fm-portal"
              substitute packaging/xdg-desktop-portal-gnilfm.service.in \
                $out/lib/systemd/user/xdg-desktop-portal-gnilfm.service \
                --replace-fail '@portal_executable@' "$out/bin/gnil-fm-portal"
              install -Dm644 assets/brand/gnil-fm.svg \
                $out/share/icons/hicolor/scalable/apps/gnil-fm.svg
              for size in 32 64 128 256 512; do
                install -Dm644 assets/brand/generated/gnil-fm-$size.png \
                  $out/share/icons/hicolor/''${size}x''${size}/apps/gnil-fm.png
              done
            '';
            postFixup = ''
              wrapProgram $out/bin/gnil-fm \
                --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath (runtimeLibraries pkgs)}
              wrapProgram $out/bin/gnil-fm-portal \
                --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath (runtimeLibraries pkgs)}
            '';
            meta = with pkgs.lib; {
              description = "A calm, keyboard-friendly file manager for Wayland";
              license = licenses.mit;
              mainProgram = "gnil-fm";
              platforms = platforms.linux;
            };
            };
          runtimeClosure = pkgs.closureInfo {
            rootPaths = [ gnilPackage ];
          };
          dynamicLinker = builtins.baseNameOf pkgs.stdenv.cc.bintools.dynamicLinker;
        in {
          default = gnilPackage;
          tarball = pkgs.runCommand "gnil-fm-0.1.0-linux.tar.gz" {
            nativeBuildInputs = [ pkgs.findutils pkgs.gnutar pkgs.gzip ];
          } ''
            bundle=staging/gnil-fm-0.1.0
            mkdir -p $bundle/bin $bundle/lib $bundle/libexec $out

            install -Dm755 ${gnilPackage}/bin/.gnil-fm-wrapped $bundle/libexec/gnil-fm
            install -Dm755 ${gnilPackage}/bin/.gnil-fm-portal-wrapped \
              $bundle/libexec/gnil-fm-portal
            cp -r ${gnilPackage}/share $bundle/
            install -Dm755 ${cleanSource}/packaging/portable-launcher.sh $bundle/bin/gnil-fm
            install -Dm755 ${cleanSource}/packaging/portable-launcher.sh \
              $bundle/bin/gnil-fm-portal
            substituteInPlace $bundle/bin/gnil-fm \
              --replace-fail '@dynamic_linker@' '${dynamicLinker}'
            substituteInPlace $bundle/bin/gnil-fm-portal \
              --replace-fail '@dynamic_linker@' '${dynamicLinker}'

            while IFS= read -r storePath; do
              if [ -d "$storePath/lib" ]; then
                find "$storePath/lib" -maxdepth 1 \( -type f -o -type l \) \
                  \( -name '*.so' -o -name '*.so.*' -o -name 'ld-linux*.so.*' \) \
                  -exec cp -Lf '{}' $bundle/lib/ ';'
              fi
            done < ${runtimeClosure}/store-paths

            test -x $bundle/lib/${dynamicLinker}
            test -f $bundle/lib/libwayland-client.so.0
            test -f $bundle/lib/libvulkan.so.1
            tar -C staging -czf $out/gnil-fm-0.1.0-${system}.tar.gz gnil-fm-0.1.0
          '';
        });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/gnil-fm";
          meta.description = "Launch gnil-fm";
        };
      });

      nixosModules = {
        default = nixosModule;
        "gnil-fm" = nixosModule;
      };

      homeManagerModules = {
        default = homeManagerModule;
        "gnil-fm" = homeManagerModule;
      };
    };
}
