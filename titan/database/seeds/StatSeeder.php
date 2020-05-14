<?php

use Illuminate\Database\Seeder;

class StatSeeder extends Seeder
{
    /**
     * Run the database seeds.
     *
     * @return void
     */
    public function run()
    {
        $stats = [
            'health' => [
                'type' => \Titan\Stat::TYPE_INTEGER,
                'description' => 'The health attribute for the user',
                'default' => 100,
            ],
            'level' => [
                'type' => \Titan\Stat::TYPE_INTEGER,
                'description' => 'The level attribute for the user',
                'default' => 1,
            ],
            'money' => [
                'type' => \Titan\Stat::TYPE_INTEGER,
                'description' => 'The money attribute for the user',
                'default' => 1000,
            ],
            'experience' => [
                'type' => \Titan\Stat::TYPE_INTEGER,
                'description' => 'The experience attribute for the user',
                'default' => 0,
            ],
            'alive' =>  [
                'type'  =>  \Titan\Stat::TYPE_BOOLEAN,
                'description'   =>  'Is this character alive?',
                'default'   =>  true
            ]
        ];

        foreach ($stats as $stat => $values) {
            $model = new \Titan\Stat();
            $model->stat = $stat;
            $model->description = $values['description'];
            $model->type = $values['type'];
            $model->default = $values['default'];
            $model->save();
        }
    }
}
