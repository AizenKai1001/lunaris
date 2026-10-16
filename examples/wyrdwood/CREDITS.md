# Credits for Wyrdwood's assets

* **The hero's model and its animations** (`models/hero.glb`): the mannequin and the animation clips of Quaternius's **Universal Animation Library** and **Universal Animation Library 2** (Standard editions), released under **CC0**
  (public domain): https://quaternius.com/packs/universalanimationlibrary.html, https://opengameart.org/node/174563, https://opengameart.org/content/universal-animation-library-2.
  The two libraries share one skeleton under different joint names: `tools/glb_retarget.py` puts the clips of the second on the first, `tools/glb_paint.py` colours the mannequin by body part (skin, tunic, trousers,
  boots), keeps the clips the game uses and packs the file. Thank you, Quaternius.
* **Trees and rocks** are the models of Caldera (`examples/caldera/models`, made by `tools/make_caldera_models.py`), the item icons are painted by `tools/make_wyrdwood_icons.py`, and the land comes from Gaea and Tellus
  (`tools/gen_wyrdwood.py`): all ours.
