import java.util.ArrayList;

public class Main {
    static class A {
        protected String tag = "A";
        String who() { return "A"; }
        String chain() { return this.who(); }
    }
    static class B extends A {
        String who() { return "B<" + super.who() + ">"; }
    }
    static class C extends B {
        String who() { return "C<" + super.who() + ">"; }
        String tagOf() { return this.tag; }
    }
    static class D extends C {
        String who() { return "D<" + super.who() + ">"; }
    }

    public static void main(String[] args) {
        System.out.println(new A().who());
        System.out.println(new B().who());
        System.out.println(new C().who());
        System.out.println(new D().who());

        A asA = new D();
        System.out.println(asA.chain());
        System.out.println(asA.who());

        System.out.println(new D().tagOf());

        ArrayList<A> all = new ArrayList<>();
        all.add(new A());
        all.add(new B());
        all.add(new C());
        all.add(new D());
        for (A x : all) {
            System.out.println(x.chain());
        }
    }
}
