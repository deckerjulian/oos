Fixed:

- The start from the stock system (the one-shot) and `card-install.py` take a card only when its partition table also
  has the stock `backup` partition, as the update already did: OOS keeps its previous image right after it.
