defmodule VecPak do
  use Rustler,
    otp_app: :vecpak_ex,
    crate: :vecpak_ex

  def encode(_term), do: :erlang.nif_error(:nif_not_loaded)
  def decode(_binary), do: :erlang.nif_error(:nif_not_loaded)
end
