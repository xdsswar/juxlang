import java.util.concurrent.Callable;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;

public class Main {
    static int square(int n) {
        return n * n;
    }

    public static void main(String[] args) throws Exception {
        ExecutorService pool = Executors.newFixedThreadPool(4);

        Future<Integer> a = pool.submit((Callable<Integer>) () -> square(3));
        Future<Integer> b = pool.submit((Callable<Integer>) () -> square(4));
        Future<Integer> c = pool.submit((Callable<Integer>) () -> square(5));
        System.out.println(a.get());
        System.out.println(b.get());
        System.out.println(c.get());

        Future<Integer> total = pool.submit((Callable<Integer>) () -> {
            int sum = 0;
            for (int i = 1; i <= 10; i++) {
                sum = sum + i;
            }
            return sum;
        });
        System.out.println(total.get());

        Future<Integer> doubled = pool.submit((Callable<Integer>) () -> square(6));
        System.out.println(doubled.get() + 1);

        pool.shutdown();
    }
}
