# Ubuntu-এর জন্য Linkwise মডেম প্রিভিউ

Linkwise দ্বিমুখী সফটওয়্যার মডেম, স্টেশন ডায়াগনস্টিক ও স্থানীয় সিমুলেশন দেয়। অডিও রাউটিং ও প্রেরণের সেটিংয়ের জন্য Station Setup দেখুন।

প্রিভিউ ইনস্টলারে `secure-transfer` নেই। gzip/SHA-256 ফাইল স্থানান্তর, যৌথ PKI, সুরক্ষিত চ্যাট ও মেল এবং সার্টিফিকেটসহ QSL কার্ডের জন্য সোর্স থেকে বিল্ড করুন। কনফিগারেশন ও হাইব্রিড চ্যাটের শর্ত [PKI নির্দেশিকায়](../../../../../../rust/docs/pki.md) আছে।

## ইনস্টল বা আপডেট

আর্কিটেকচার জানতে `dpkg --print-architecture` চালান। ডাউনলোড ডিরেক্টরিতে নিচের কমান্ডগুলো চালিয়ে উদাহরণের বদলে মেলে এমন প্যাকেজের ফাইলের নাম ব্যবহার করুন।

```sh
sudo apt install ./linkwise-modem_0.1.0_arm64.deb
linkwise-modemd --version
linkwise-modemd --simulate
```

APT প্রয়োজনীয় সিস্টেম লাইব্রেরি ইনস্টল করে। আপডেটে একই ইনস্টল কমান্ডের সঙ্গে নতুন ফাইল দিন। সিমুলেশন স্থানীয়ভাবে রেডিও ছাড়া চলে।

## স্টেশন সেট আপ করুন

কমান্ডের জন্য `man 8 linkwise-modemd` এবং বিন্যাসের জন্য `man 5 linkwise-station` দেখুন। ডিভাইস নির্বাচন ও `station.json` তৈরির জন্য `/usr/share/doc/linkwise-modem/Station-setup.md` খুলুন। বর্তমান অডিও সেশনে প্রবেশের জন্য ডেস্কটপ ব্যবহারকারী হিসেবে পরীক্ষা চালান।

## মডেম সেবা চালান

ডেস্কটপ স্টেশনের কনফিগারেশন `~/.config/linkwise/station.json`-এ সেভ করে নিচের কমান্ড চালান।

```sh
systemctl --user enable --now linkwise-modem.service
journalctl --user -u linkwise-modem.service
```

নির্দিষ্ট স্টেশনের জন্য প্রশাসকের মালিকানাধীন কনফিগারেশন ও প্রোফাইল `/etc/linkwise`-এ রাখুন। সেবাকে ফাইল পড়তে এবং কনফিগার করা ALSA ইনপুট ও আউটপুট সরাসরি ব্যবহার করতে দিতে হবে। একই ডিভাইস ও পোর্ট জোড়ার জন্য একটি সেবা চালান।

```sh
sudo systemctl enable --now linkwise-modem.service
sudo journalctl -u linkwise-modem.service
```

`bandwidth` দিলে মডেম IPv4 লুপব্যাকে দুই পোর্ট খোলে। ক্ষেত্রটি না থাকলে কমান্ড পোর্ট JSON-এ রিসিভ ডায়াগনস্টিক দেয়। `nc 127.0.0.1 8300` দিয়ে পড়ুন।

## আনইনস্টল

চলমান Linkwise কমান্ড বন্ধ করে `sudo apt remove linkwise-modem` চালান। হোম ডিরেক্টরির স্টেশন ফাইলগুলো থাকবে।
