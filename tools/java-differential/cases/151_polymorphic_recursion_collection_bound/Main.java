import java.util.*;

public class Main {
    interface Rel<L> { int weigh(L list); }

    static class Item implements Rel<List<Item>> {
        int w;
        Item(int w) { this.w = w; }
        public int weigh(List<Item> list) { list.add(this); return w + list.size(); }
    }

    static class Crate<T extends Rel<List<T>>> implements Rel<List<Crate<T>>> {
        T inner;
        Crate(T inner) { this.inner = inner; }
        public int weigh(List<Crate<T>> list) {
            List<T> mine = new ArrayList<>();
            int got = inner.weigh(mine);
            list.add(this);
            return 1 + got + mine.size() * 10 + list.size() * 100;
        }
    }

    static <T extends Rel<List<T>>> int pack(T x, int n) {
        if (n == 0) {
            List<T> seen = new ArrayList<>();
            int r = x.weigh(seen);
            int again = x.weigh(seen);
            return r * 1000 + again + seen.size() * 1000000;
        }
        return pack(new Crate<T>(x), n - 1);
    }

    interface Keeper<L> {
        void keep(L list);
        void addLater();
        L made();
    }

    interface Tally<M> { int tally(M byName); }

    static class Leaf implements Keeper<List<Leaf>>, Tally<Map<String, Leaf>> {
        int id;
        List<Leaf> kept = null;
        List<Leaf> mine = new ArrayList<>();
        Leaf(int id) { this.id = id; }
        public void keep(List<Leaf> list) { this.kept = list; }
        public void addLater() { this.kept.add(this); this.mine.add(this); }
        public List<Leaf> made() { return this.mine; }
        public int tally(Map<String, Leaf> byName) {
            byName.put("leaf" + id, this);
            return byName.size();
        }
    }

    static class Wrap<T extends Keeper<List<T>> & Tally<Map<String, T>>>
            implements Keeper<List<Wrap<T>>>, Tally<Map<String, Wrap<T>>> {
        T inner;
        List<Wrap<T>> kept = null;
        List<Wrap<T>> mine = new ArrayList<>();
        Wrap(T inner) { this.inner = inner; }
        public void keep(List<Wrap<T>> list) { this.kept = list; }
        public void addLater() { this.kept.add(this); this.mine.add(this); }
        public List<Wrap<T>> made() { return this.mine; }
        public int tally(Map<String, Wrap<T>> byName) {
            byName.put("wrap", this);
            return 100 + byName.size();
        }
    }

    static <T extends Keeper<List<T>> & Tally<Map<String, T>>> void run(T x, int n) {
        if (n == 0) {
            List<T> list = new ArrayList<>();
            x.keep(list);
            System.out.println("before: " + list.size());
            x.addLater();
            x.addLater();
            System.out.println("after: " + list.size());
            List<T> made = x.made();
            System.out.println("made: " + made.size());
            x.addLater();
            System.out.println("made again: " + made.size() + " " + list.size());
            made.clear();
            x.addLater();
            System.out.println("cleared then one: " + x.made().size());
            Map<String, T> names = new HashMap<>();
            names.put("first", x);
            int t = x.tally(names);
            System.out.println("tally: " + t + " " + names.size());
            System.out.println("same: " + (list.get(0) == x));
            return;
        }
        run(new Wrap<T>(x), n - 1);
    }

    public static void main(String[] args) {
        System.out.println(pack(new Item(4), 0));
        System.out.println(pack(new Item(4), 3));
        run(new Leaf(1), 0);
        run(new Leaf(2), 2);
    }
}
