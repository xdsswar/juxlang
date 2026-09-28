import java.util.ArrayList;

public class Main {
    interface Visitor<R> {
        R visit(String what);
    }

    static abstract class Tree<T> {
        public abstract <R> R accept(Visitor<R> v);
    }

    static class Weird<T, U> extends Tree<T> {
        T head;
        U extra;
        Weird(T head, U extra) {
            this.head = head;
            this.extra = extra;
        }
        public <R> R accept(Visitor<R> v) {
            return v.visit(head + " / " + extra);
        }
    }

    static <A, B> Tree<A> grow(A a, B b, int n) {
        System.out.println("depth " + n + ": " + b);
        if (n == 0) {
            return new Weird<A, B>(a, b);
        }
        ArrayList<B> wrapped = new ArrayList<B>();
        wrapped.add(b);
        wrapped.add(b);
        return Main.<A, ArrayList<B>>grow(a, wrapped, n - 1);
    }

    static class Show implements Visitor<String> {
        public String visit(String what) {
            return "visited " + what;
        }
    }

    static class Size implements Visitor<Integer> {
        public Integer visit(String what) {
            return what.length();
        }
    }

    public static void main(String[] args) {
        Tree<Integer> t = Main.<Integer, String>grow(1, "x", 5);
        System.out.println(t.accept(new Show()));
        System.out.println(t.accept(new Size()));

        Weird<Integer, String> w = new Weird<Integer, String>(7, "y");
        System.out.println(w.head + 1);
        System.out.println(w.extra + "!");
        w.head = 9;
        System.out.println(w.head * 2);
        System.out.println(w.accept(new Show()));
    }
}
