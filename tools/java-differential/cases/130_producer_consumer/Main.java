import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.BlockingQueue;

public class Main {
    record Job(int id, long cost) {}

    static final Job END = new Job(-1, 0);

    static class Batcher {
        private final List<Job> pending = new ArrayList<>();
        private final int size;
        int batches = 0;
        long total = 0;

        Batcher(int size) { this.size = size; }

        void add(Job j) {
            pending.add(j);
            if (pending.size() == size) {
                flush();
            }
        }

        void flush() {
            if (pending.isEmpty()) {
                return;
            }
            long sum = 0;
            String ids = "";
            for (Job j : pending) {
                sum += j.cost();
                ids = ids + (ids.length() == 0 ? "" : ",") + j.id();
            }
            batches++;
            total += sum;
            System.out.println("batch " + batches + " [" + ids + "] cost " + sum);
            pending.clear();
        }
    }

    static void pipeline(int jobs, int batchSize) throws InterruptedException {
        BlockingQueue<Job> ch = new ArrayBlockingQueue<>(2);
        Thread producer = new Thread(() -> {
            try {
                for (int i = 1; i <= jobs; i++) {
                    ch.put(new Job(i, i * 7L % 10));
                }
                ch.put(END);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            }
        });
        producer.start();
        Batcher b = new Batcher(batchSize);
        int received = 0;
        while (true) {
            Job next = ch.take();
            if (next == END) {
                break;
            }
            received++;
            b.add(next);
        }
        b.flush();
        producer.join();
        System.out.println("received " + received + " in " + b.batches + " batches, total " + b.total);
    }

    public static void main(String[] args) throws InterruptedException {
        pipeline(7, 3);
        pipeline(0, 4);
    }
}
