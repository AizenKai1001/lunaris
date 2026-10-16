"""The inventory, the hotbar, crafting and the use of tools of Wyrdwood as Lunaris Script, made from the item table (`tools/wyrdwood_items.py`), and the entities of their interface.

Inventory slots are resources: `$inv.N.id` (the item number, 0 = empty), `$inv.N.n` (how many) and `$inv.N.dur` (what is left of its durability) for N = 0 to 31 (row 0 is the hotbar). The script language has no loops and
no variable index, so every operation over the slots is written out slot by slot here: that is what this module is for. Systems, in the order they run:

  6  inv_totals     `$have.KEY`: how many of every item are held; `$can.R`: can recipe R be made now (what it takes, and the station it needs)
  7  inv_select     the hotbar keys: select a slot (a food is eaten), and what is held: `$held.id`, `.class` (1 axe, 2 pick, 3 hammer, 4 weapon, 5 torch), `.power`
  9  craft          a click on a recipe asks for the ingredients to be taken and the result to be given
  10 inv_remove     takes what was asked for (`$rm.K.id`, `$rm.K.n`, four requests at a time)
  11 inv_give       puts what was given (`$give.id`, `$give.n`) into the slots, on a stack of the same thing first, then the first empty slot
  20 slot interface the picture of every slot, and a click on a slot picks its item up and puts it down in another (swapping)
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))  # `python -I` leaves the script's folder out
import wyrdwood_items as I  # noqa: E402

N = I.INV_SLOTS
HOT = I.HOT_SLOTS
CLASS_NUM = {"axe": 1, "pick": 2, "hammer": 3, "weapon": 4, "torch": 5, "food": 6, "trophy": 7, "armor": 8, "res": 0}


def f(x):
    return f"{x:.6g}"


def chain(var, table, default="0"):
    """A select chain: `table` maps item numbers to expressions (as text)."""
    expr = default
    for k, v in sorted(table.items(), reverse=True):
        expr = f"select({var} == {k}, {v}, {expr})"
    return expr


def stack_of(idv):
    return chain(idv, {num: it[2] for num, it in I.BY_ID.items()}, "50")


def dur_of(idv):
    return chain(idv, {num: it[5] for num, it in I.BY_ID.items() if it[5]}, "0")


def slot_pick(var, field, count=N):
    """The value of `$inv.<var>.<field>` for the slot number in `var` (a select chain over all slots)."""
    expr = "0"
    for i in range(count - 1, -1, -1):
        expr = f"select({var} == {i}, $inv.{i}.{field}, {expr})"
    return expr


# ---------------------------------------------------------------------------------------------- adding and removing
# A system reads the resources as they were when it started and its writes apply when it is over: so what is left to add or to take is carried from slot to slot in `let`s (which cannot change), and the slots are
# written once at the end.
def add_code(tag, idv, nv):
    """Script that puts `nv` of item `idv` into the slots (stacks of it first, then empty slots); `{tag}_left` is what did not fit."""
    s = [f"    let {tag}_stack = {stack_of(idv)};", f"    let {tag}_dur = {dur_of(idv)};", f"    let {tag}_r0 = {nv};"]
    for i in range(N):
        s.append(f"    let {tag}_a{i} = select({tag}_r{i} > 0 && $inv.{i}.id == {idv} && $inv.{i}.n < {tag}_stack, min({tag}_r{i}, {tag}_stack - $inv.{i}.n), 0);")
        s.append(f"    let {tag}_r{i + 1} = {tag}_r{i} - {tag}_a{i};")
    s.append(f"    let {tag}_q0 = {tag}_r{N};")
    for i in range(N):
        s.append(f"    let {tag}_e{i} = select({tag}_q{i} > 0 && $inv.{i}.id == 0, min({tag}_q{i}, {tag}_stack), 0);")
        s.append(f"    let {tag}_q{i + 1} = {tag}_q{i} - {tag}_e{i};")
    s.append(f"    let {tag}_left = {tag}_q{N};")
    for i in range(N):
        s.append(f"    if {tag}_a{i} + {tag}_e{i} > 0 {{")
        s.append(f"        $inv.{i}.n = $inv.{i}.n + {tag}_a{i} + {tag}_e{i};")
        s.append("    }")
        s.append(f"    if {tag}_e{i} > 0 {{")
        s.append(f"        $inv.{i}.id = {idv};")
        s.append(f"        $inv.{i}.dur = {tag}_dur;")
        s.append("    }")
    return "\n".join(s) + "\n"


def remove_code(tag, idv, nv):
    """Script that takes `nv` of item `idv` out of the slots (the first slots that hold it first)."""
    s = [f"    let {tag}_r0 = {nv};"]
    for i in range(N):
        s.append(f"    let {tag}_t{i} = select({tag}_r{i} > 0 && $inv.{i}.id == {idv}, min({tag}_r{i}, $inv.{i}.n), 0);")
        s.append(f"    let {tag}_r{i + 1} = {tag}_r{i} - {tag}_t{i};")
    for i in range(N):
        s.append(f"    if {tag}_t{i} > 0 {{")
        s.append(f"        $inv.{i}.n = $inv.{i}.n - {tag}_t{i};")
        s.append(f"        if $inv.{i}.n - {tag}_t{i} <= 0 {{")
        s.append(f"            $inv.{i}.id = 0;")
        s.append(f"            $inv.{i}.dur = 0;")
        s.append("        }")
        s.append("    }")
    return "\n".join(s) + "\n"


# ---------------------------------------------------------------------------------------------- the systems
def systems():
    out = []
    # ---- totals and what can be made
    t = ["system inv_totals each Hero order 6 {"]
    for num, it in I.BY_ID.items():
        terms = " + ".join(f"select($inv.{i}.id == {num}, $inv.{i}.n, 0)" for i in range(N))
        t.append(f"    $have.{it[0]} = {terms};")
    t.append("    let wb = $station.wb;")
    for r, (res, count, station, ingr) in enumerate(I.RECIPES):
        need = " && ".join(f"$have.{k} >= {n}" for k, n in ingr)
        if station == "workbench":
            need += " && wb > 0.5"
        t.append(f"    $can.{r} = select({need}, 1, 0);")
    t.append("}")
    out.append("\n".join(t))
    # ---- the hotbar and what is held
    h = ["system inv_select each Hero order 7 {", "    let busy = $invopen > 0 || $paused > 0;"]
    for i in range(HOT):
        h.append(f"    if $action.hot{i + 1}.pressed > 0.5 && !busy {{")
        h.append(f"        $hot = {i};")
        h.append(f"        $eat.try = {i + 1};")
        h.append("    }")
    h.append(f"    let hid = {slot_pick('$hot', 'id', HOT)};")
    h.append("    $held.id = hid;")
    h.append(f"    $held.class = {chain('hid', {num: CLASS_NUM[it[3]] for num, it in I.BY_ID.items()})};")
    h.append(f"    $held.power = {chain('hid', {num: it[4] for num, it in I.BY_ID.items()})};")
    h.append("}")
    out.append("\n".join(h))
    # ---- crafting
    c = ["system craft each RecipeBtn, Ui order 9 {", "    if Ui.clicked && $invopen > 0 {"]
    for r, (res, count, station, ingr) in enumerate(I.RECIPES):
        c.append(f"        if RecipeBtn.n == {r} && $can.{r} > 0.5 && $give.n == 0 && $rm.0.n == 0 {{")
        for k, (key, n) in enumerate(ingr):
            c.append(f"            $rm.{k}.id = {I.ID[key]};")
            c.append(f"            $rm.{k}.n = {n};")
        c.append(f"            $give.id = {I.ID[res]};")
        c.append(f"            $give.n = {count};")
        c.append("            emit \"craft\";")
        c.append("        }")
    c.append("    }")
    c.append("}")
    out.append("\n".join(c))
    # ---- removing what was asked for
    r = ["system inv_remove each Hero order 10 {"]
    for k in range(4):
        r.append(f"    if $rm.{k}.n > 0 {{")
        r.append(remove_code(f"rm{k}", f"$rm.{k}.id", f"$rm.{k}.n"))
        r.append(f"        $rm.{k}.n = 0;")
        r.append(f"        $rm.{k}.id = 0;")
        r.append("    }")
    r.append("}")
    out.append("\n".join(r))
    # ---- giving
    g = ["system inv_give each Hero order 11 {", "    if $give.n > 0 {", add_code("gv", "$give.id", "$give.n"), "        $give.n = 0;", "        $give.id = 0;", "    }", "}"]
    out.append("\n".join(g))
    # ---- wear of the tool in the hand: once per swing that hit something
    w = ["system inv_wear each Hero order 25 {", "    if $hitfx > 0 {", "        $hitfx = 0;"]
    for i in range(HOT):
        w.append(f"        if $hot == {i} && $inv.{i}.dur > 0 {{")
        w.append(f"            $inv.{i}.dur -= 1;")
        w.append(f"            if $inv.{i}.dur <= 0 {{")
        w.append(f"                $inv.{i}.id = 0;")
        w.append(f"                $inv.{i}.n = 0;")
        w.append("                emit \"break\";")
        w.append("            }")
        w.append("        }")
    w.append("    }")
    w.append("}")
    out.append("\n".join(w))
    # ---- the picture of every slot, and the clicks
    for i in range(N):
        sh = [f"system slot_ui_{i} each InvSlot, Ui order 20 {{", f"    if InvSlot.n == {i} {{"]
        sh.append(f"        let shown = {'1' if i < HOT else '$invopen'};")
        sh.append("        Ui.visible = shown > 0;")
        sh.append(f"        let sel = select($hot == {i}, 1, 0);")
        sh.append(f"        let carried = select($mv == {i + 1}, 1, 0);")
        sh.append(f"        let has = select($inv.{i}.id > 0, 1, 0);")
        sh.append("        Ui.color[0] = 0.06 + 0.5 * sel + 0.25 * carried;")
        sh.append("        Ui.color[1] = 0.07 + 0.38 * sel + 0.2 * carried;")
        sh.append("        Ui.color[2] = 0.09 + 0.15 * sel;")
        sh.append("        Ui.color[3] = 0.78 + 0.2 * sel;")
        sh.append("        Ui.border_color[0] = 0.7 + 0.3 * sel;")
        sh.append("        Ui.border_color[1] = 0.6 + 0.35 * sel;")
        sh.append("        Ui.border_color[2] = 0.45 + 0.2 * sel;")
        sh.append("        Ui.border_color[3] = 0.55 + 0.45 * sel + 0.3 * carried;")
        sh.append("        if Ui.clicked && $invopen > 0 {")
        sh.append("            if $mv == 0 {")
        sh.append(f"                if $inv.{i}.id > 0 {{")
        sh.append(f"                    $mv = {i + 1};")
        sh.append("                }")
        sh.append("            } else {")
        sh.append("                $sw.from = $mv;")
        sh.append(f"                $sw.to = {i + 1};")
        sh.append("                $mv = 0;")
        sh.append("            }")
        sh.append("        }")
        sh.append("        if Ui.clicked && $invopen == 0 {")
        if i < HOT:
            sh.append(f"            $hot = {i};")
        sh.append("        }")
        sh.append("    }")
        sh.append("}")
        out.append("\n".join(sh))
        ic = [f"system slot_icon_{i} each InvIcon, Ui, UiStyle order 21 {{", f"    if InvIcon.n == {i} {{"]
        ic.append(f"        let id = $inv.{i}.id;")
        ic.append(f"        let shown = {'1' if i < HOT else '$invopen'};")
        ic.append("        Ui.visible = shown > 0 && id > 0;")
        ic.append("        let col = id - floor(id / 8) * 8;")
        ic.append("        let row = floor(id / 8);")
        ic.append("        UiStyle.image_uv[0] = col / 8;")
        ic.append("        UiStyle.image_uv[1] = row / 8;")
        ic.append("        UiStyle.image_uv[2] = (col + 1) / 8;")
        ic.append("        UiStyle.image_uv[3] = (row + 1) / 8;")
        ic.append("    }")
        ic.append("}")
        out.append("\n".join(ic))
        ct = [f"system slot_count_{i} each InvCount, Text order 21 {{", f"    if InvCount.n == {i} {{"]
        ct.append(f"        let shown = {'1' if i < HOT else '$invopen'};")
        ct.append(f"        Text.visible = shown > 0 && $inv.{i}.n > 1;")
        ct.append("    }")
        ct.append("}")
        out.append("\n".join(ct))
    # swapping two slots (a click on one slot after a click on another)
    sw = ["system inv_swap each Hero order 12 {", "    if $sw.to > 0 && $sw.from > 0 && $sw.from != $sw.to {"]
    sw.append(f"        let fi = {slot_pick('($sw.from - 1)', 'id')};")
    sw.append(f"        let fn = {slot_pick('($sw.from - 1)', 'n')};")
    sw.append(f"        let fd = {slot_pick('($sw.from - 1)', 'dur')};")
    sw.append(f"        let ti = {slot_pick('($sw.to - 1)', 'id')};")
    sw.append(f"        let tn = {slot_pick('($sw.to - 1)', 'n')};")
    sw.append(f"        let td = {slot_pick('($sw.to - 1)', 'dur')};")
    # a stack of the same thing is merged instead of swapped
    sw.append(f"        let room = {stack_of('fi')} - tn;")
    sw.append("        let same = select(fi == ti && fi > 0, 1, 0);")
    sw.append("        let moved = select(same > 0, min(fn, room), 0);")
    for j in range(N):
        sw.append(f"        if $sw.from == {j + 1} {{")
        sw.append("            if same > 0 {")
        sw.append(f"                $inv.{j}.n = fn - moved;")
        sw.append(f"                if $inv.{j}.n <= 0 {{")
        sw.append(f"                    $inv.{j}.id = 0;")
        sw.append(f"                    $inv.{j}.dur = 0;")
        sw.append("                }")
        sw.append("            } else {")
        sw.append(f"                $inv.{j}.id = ti;")
        sw.append(f"                $inv.{j}.n = tn;")
        sw.append(f"                $inv.{j}.dur = td;")
        sw.append("            }")
        sw.append("        }")
        sw.append(f"        if $sw.to == {j + 1} {{")
        sw.append("            if same > 0 {")
        sw.append(f"                $inv.{j}.n = tn + moved;")
        sw.append("            } else {")
        sw.append(f"                $inv.{j}.id = fi;")
        sw.append(f"                $inv.{j}.n = fn;")
        sw.append(f"                $inv.{j}.dur = fd;")
        sw.append("            }")
        sw.append("        }")
    sw.append("    }")
    sw.append("    $sw.to = 0;")
    sw.append("    $sw.from = 0;")
    sw.append("}")
    out.append("\n".join(sw))
    # the panel, the cursor and the recipe list
    out.append("""system inv_toggle each Hero order 5 {
    if $action.inventory.pressed > 0.5 && $paused == 0 {
        $invopen = 1 - $invopen;
        $mv = 0;
        emit "select";
    }
}

system inv_panel each InvPanel, Ui order 21 {
    Ui.visible = $invopen > 0;
}

system inv_panel_text each InvPanel, Text order 21 {
    Text.visible = $invopen > 0;
}

system recipe_ui each RecipeBtn, Ui order 21 {
    Ui.visible = $invopen > 0;
    let ok = %s;
    Ui.color[0] = lerp(0.07, 0.1, ok);
    Ui.color[1] = lerp(0.08, 0.2, ok);
    Ui.color[2] = lerp(0.09, 0.12, ok);
    Ui.text_color[0] = lerp(0.5, 1, ok);
    Ui.text_color[1] = lerp(0.5, 0.95, ok);
    Ui.text_color[2] = lerp(0.5, 0.8, ok);
}

system recipe_icon each RecipeIcon, Ui order 21 {
    Ui.visible = $invopen > 0;
}
""" % (chain("RecipeBtn.n", {r: f"$can.{r}" for r in range(len(I.RECIPES))}, "0")))
    return "\n".join(out)


SCRIPT = systems()

if __name__ == "__main__":
    print(len(SCRIPT), "characters")
    print(SCRIPT[:1500])
