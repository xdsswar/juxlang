import java.util.concurrent.CompletableFuture;

public class Main {
    static CompletableFuture<Integer> slow(int n) {
        System.out.println("enter " + n);
        return CompletableFuture.completedFuture(n * 2);
    }

    static CompletableFuture<Integer> chained(int n) {
        int a = slow(n).join();
        System.out.println("mid " + a);
        int b = slow(a).join();
        return CompletableFuture.completedFuture(b);
    }

    static CompletableFuture<String> label(String tag) {
        return CompletableFuture.completedFuture("[" + tag + "]");
    }

    public static void main(String[] args) {
        System.out.println("start");
        int r = chained(3).join();
        System.out.println("chained " + r);

        int x = slow(1).join();
        int y = slow(x).join();
        System.out.println("seq " + x + " " + y);

        System.out.println(label("a").join() + label("b").join());

        int sum = slow(2).join() + slow(5).join();
        System.out.println("sum " + sum);
        System.out.println("done");
    }
}
