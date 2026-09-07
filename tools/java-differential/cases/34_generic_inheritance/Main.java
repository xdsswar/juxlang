public class Main {
    static class Store<T> {
        protected T item;
        Store(T item) { this.item = item; }
        T get() { return this.item; }
        String tag() { return "store:" + this.item; }
    }
    static class IntStore extends Store<Integer> {
        IntStore(int v) { super(v); }
        int doubled() { return this.get() * 2; }
        String tag() { return "int:" + this.get(); }
    }
    static class Loud<T> extends Store<T> {
        Loud(T v) { super(v); }
        String shout() { return "!" + this.get() + "!"; }
    }

    public static void main(String[] args) {
        Store<String> s = new Store<>("a");
        System.out.println(s.get());
        System.out.println(s.tag());

        IntStore i = new IntStore(21);
        System.out.println(i.get());
        System.out.println(i.doubled());
        System.out.println(i.tag());

        Loud<String> l = new Loud<>("hey");
        System.out.println(l.get());
        System.out.println(l.shout());
        System.out.println(l.tag());

        Store<Integer> base = new IntStore(5);
        System.out.println(base.tag());
        System.out.println(base.get());

        Loud<Store<Integer>> ln = new Loud<>(new Store<>(8));
        System.out.println(ln.get().get());
        System.out.println(ln.get().tag());
    }
}
