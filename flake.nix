{
  description = "ESP32 development environment";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
    flake-utils.url = "github:numtide/flake-utils";
    esp-dev = {
      url = "github:mirrexagon/nixpkgs-esp-dev";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-utils.follows = "flake-utils";
    };
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, esp-dev, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [
            esp-dev.overlays.default
            (import rust-overlay)
          ];
          config.permittedInsecurePackages = [
            "python3.13-ecdsa-0.19.1"
          ];
        };
        esp-idf = pkgs.esp-idf-full.override {
          rev = "v5.4.3";
          sha256 = "sha256-sV/eL3jRG9GdaQNByBypmH5ZKmZoOnWCEY1ABySIeac=";
        };
      in {
        devShells.default = pkgs.mkShell {
          inputsFrom = [ esp-idf ];
          packages = with pkgs; [
            esp-idf
            (rust-bin.stable.latest.default.override {
              extensions = [ "rust-src" ];
            })
            pkg-config
          ];
          shellHook = ''
            export ESP_IDF_VERSION="5.4"
          '';
        };
      }
    );
}
