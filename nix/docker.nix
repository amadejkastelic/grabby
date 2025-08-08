{
  pkgs,
  grabby,
}:
pkgs.dockerTools.buildLayeredImage {
  name = "grabby";
  tag = "latest";
  created = "now";

  maxLayers = 10;

  contents = [
    # Layer 1: dependencies
    pkgs.cacert

    # Layer 2: app
    grabby
  ];

  config = {
    Entrypoint = [ "grabby" ];
    Env = [
      "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
    ];
  };
}
