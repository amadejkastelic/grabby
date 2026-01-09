{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.grabby;

  tomlFormat = pkgs.formats.toml { };

  configFile = tomlFormat.generate "grabby-config.toml" {
    logging.level = cfg.logLevel;
    servers = map (server: {
      server_id = server.serverId;
      auto_embed_channels = server.autoEmbedChannels;
      embed_enabled = server.embedEnabled;
    }) cfg.servers;
  };
in
{
  options.services.grabby = {
    enable = lib.mkEnableOption "Grabby - Media Embedding Discord Bot";

    package = lib.mkPackageOption pkgs "grabby" { };

    environmentFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Path to file containing environment variables (e.g., DISCORD_TOKEN), compatible with sops-nix";
      example = "/run/secrets/grabby-env";
    };

    logLevel = lib.mkOption {
      type = lib.types.enum [
        "error"
        "warn"
        "info"
        "debug"
        "trace"
      ];
      default = "info";
      description = "Log level for the grabby bot";
    };

    servers = lib.mkOption {
      type = lib.types.listOf (
        lib.types.submodule {
          options = {
            serverId = lib.mkOption {
              type = lib.types.str;
              description = "Discord server ID";
              example = "123456789";
            };

            autoEmbedChannels = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [ ];
              description = "List of channel IDs where auto-embed is enabled";
              example = [
                "channel1"
                "channel2"
              ];
            };

            embedEnabled = lib.mkOption {
              type = lib.types.bool;
              default = true;
              description = "Enable embed for this server";
            };
          };
        }
      );

      default = [ ];
      description = "List of server configurations";
      example = [
        {
          serverId = "123456789";
          autoEmbedChannels = [
            "channel1"
            "channel2"
          ];
          embedEnabled = true;
        }
      ];
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "grabby";
      description = "User account under which grabby runs";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "grabby";
      description = "Group under which grabby runs";
    };
  };

  config = lib.mkIf cfg.enable {
    users.users.${cfg.user} = lib.mkIf (cfg.user == "grabby") {
      description = "Grabby Discord bot user";
      isSystemUser = true;
      group = cfg.group;
    };

    users.groups.${cfg.group} = lib.mkIf (cfg.group == "grabby") { };

    systemd.services.grabby = {
      description = "Grabby - Media Embedding Discord Bot";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];

      serviceConfig = {
        User = cfg.user;
        Group = cfg.group;
        Restart = "on-failure";
        RestartSec = "5s";
        ExecStart = "${cfg.package}/bin/grabby --config ${configFile}";
        EnvironmentFile = lib.mkIf (cfg.environmentFile != null) cfg.environmentFile;

        # Security hardening
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ReadWritePaths = [ "/var/lib/grabby" ];
      };
    };

    environment.systemPackages = [ cfg.package ];
  };
}
