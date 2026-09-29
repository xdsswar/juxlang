import java.util.*;
import java.util.function.*;

public class Main {
    static class Node {
        int v = 0;
        void bump() { v += 1; }
    }

    public static void main(String[] args) {
        List<Integer> v = new ArrayList<>();
        Supplier<List<Integer>> gv = () -> v;
        gv.get().add(1);
        List<Integer> got = gv.get();
        got.add(2);
        System.out.println(v.size());

        Node a = new Node();
        Supplier<Node> gn = () -> a;
        gn.get().bump();
        System.out.println(a.v);

        IntUnaryOperator sq = (int k) -> k * k;
        System.out.println(sq.applyAsInt(5) + 1);

        IntBinaryOperator add = (x, y) -> x + y;
        System.out.println(add.applyAsInt(3, 4));

        Function<Boolean, List<Integer>> pick = (Boolean first) -> {
            if (first) { return v; }
            return new ArrayList<>();
        };
        pick.apply(true).add(3);
        pick.apply(false).add(99);
        System.out.println(v.size());

        Runnable say = () -> { System.out.println("hi"); };
        say.run();

        Supplier<String> name = () -> "x" + v.size();
        System.out.println(name.get().length());
    }
}
