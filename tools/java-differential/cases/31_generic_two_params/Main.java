public class Main {
    static class Pair<A, B> {
        private A a;
        private B b;
        Pair(A a, B b) { this.a = a; this.b = b; }
        A left() { return this.a; }
        B right() { return this.b; }
        Pair<B, A> swap() { return new Pair<>(this.b, this.a); }
        String show() { return "(" + this.a + "," + this.b + ")"; }
    }

    public static void main(String[] args) {
        Pair<Integer, String> p = new Pair<>(1, "x");
        System.out.println(p.show());
        System.out.println(p.swap().show());
        System.out.println(p.left());
        System.out.println(p.right());

        Pair<Pair<Integer, Integer>, String> nested =
            new Pair<>(new Pair<>(1, 2), "tag");
        System.out.println(nested.left().show());
        System.out.println(nested.right());
        System.out.println(nested.swap().right().show());

        Pair<String, Pair<String, Pair<Integer, Integer>>> deep =
            new Pair<>("a", new Pair<>("b", new Pair<>(3, 4)));
        System.out.println(deep.left());
        System.out.println(deep.right().left());
        System.out.println(deep.right().right().show());
    }
}
